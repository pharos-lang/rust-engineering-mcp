# W28c — arnés de clientes: sesión runtime del Inspector primero, recibo incremental, turnos con timeout registrados

Worker: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`). Sin subagentes, sin segundo plano, sin commit, sin ejecutar `--run --with-runtime` completo. Archivos tocados: `scripts/test-m8-clients.py`, `scripts/test-m8-clients-unit.py`.

## Evidencia de partida

`docs/validation/M8/clients/attempt-20/receipt.json`: en `--run --with-runtime`, el turno de Codex se quedó 600 s tras `initialize`/`tools/list` sin invocar tools (sin `codex-events.jsonl`), `subprocess.TimeoutExpired` propagó sin capturar y mató el arnés **antes** de que corriera la sesión `runtime` del Inspector, dejando `receipt.json` con `status: "failed"` pero sin ningún bloque `inspector.runtime`, `eof_gate` ni `composed_prior_positives`.

## Cambios en `scripts/test-m8-clients.py`

1. **Orden en `run()`**: la sesión `docker_free` del Inspector corre primero, luego (si `--with-runtime`) la sesión `runtime`, y solo entonces los turnos de modelo Codex → Claude Code → Gemini CLI. La evidencia determinista y de mayor autoridad queda protegida de un turno de modelo que cuelgue.

2. **Recibo incremental**: nueva función `save_receipt(attempt, receipt, final)` que escribe `receipt.json` (no exclusivo, para poder sobrescribir) tras cada bloque — ambas sesiones del Inspector, cada turno de modelo, y el bloque `eof_gate`/`composed_prior_positives` en `--with-runtime`. Toda escritura salvo la última fija `status: "running"`. La escritura final (en el `finally` existente) usa `save_receipt(..., final=True)`, ya no exclusiva porque el archivo ya existe desde las escrituras incrementales.

3. **Turnos de modelo resilientes a timeout/excepción**: tres funciones nuevas (`codex_turn`, `claude_turn`, `gemini_turn`) envuelven `codex_gate`/`claude_gate`/`gemini_gate` respectivamente. Cualquier `subprocess.TimeoutExpired` o excepción se captura y se traduce a `{"status"|"classification": "unavailable", "reason": "timeout after N s" | "<TypoDeExcepción>: <mensaje>"}`; el arnés continúa con los turnos restantes en vez de abortar. `run()` llama a estos envoltorios en vez de a las funciones `_gate` directamente. El `status` global sigue exigiendo Inspector (ambos modos) + Codex `classification == "passed"`; un turno `unavailable` de Codex sigue fallando el gate, como antes — solo cambia que ahora el arnés termina limpio con un recibo completo en vez de morir con una excepción sin capturar.

4. **Timeout de Codex en modo runtime → 900 s**: `codex_gate` acepta un parámetro `timeout` (por defecto 600 s, documentado en su docstring); `run()` calcula `codex_timeout = 900 if with_runtime else 600` y lo pasa a `codex_turn`. Un socket Docker real convierte `rust.project.inspect` en una llamada de contenedor real en vez de un rechazo de host, así que el turno del modelo necesita más margen.

5. **`run()` devuelve 1 si `failed`**: ya lo hacía (`return 0 if receipt["status"] == "passed" else 1`); el cambio real es que ahora efectivamente *llega* a ese `return` cuando un turno de modelo expira, en vez de propagar la excepción sin capturar como antes.

No se tocó `scripts/m8-inspector-session.mjs` ni ningún otro archivo (ya aparecía modificado en el árbol de trabajo por otro cambio ajeno a este encargo).

## Pruebas unitarias añadidas en `scripts/test-m8-clients-unit.py` (dobles, sin clientes reales)

- `HarnessOrderTests`: con y sin `--with-runtime`, confirma que ambas sesiones del Inspector corren antes que Codex/Claude Code/Gemini CLI, y que sin runtime la sesión `docker_free` sigue precediendo a Codex.
- `IncrementalReceiptTests`: `save_receipt` marca `status: "running"` en toda escritura no final y preserva el `status` real en la final; `run()` (con todos los gates mockeados) escribe el recibo tras cada bloque (Inspector, cada turno de modelo, más la copia `CURRENT` al pasar).
- `ModelTurnResilienceTests`: `codex_turn`/`claude_turn`/`gemini_turn` clasifican un `subprocess.TimeoutExpired` como `"timeout after N s"` y cualquier otra excepción con su propio mensaje; `run()` sigue ejecutando Claude Code y Gemini CLI y devuelve `1` (sin excepción) cuando Codex expira; `run()` pasa `900` a `codex_gate` con `--with-runtime` y `600` sin él.

`python3 -B scripts/test-m8-clients-unit.py`: 128/128 tests OK (110 preexistentes + 18 nuevas). `python3 -m py_compile` limpio en ambos archivos.

## No ejecutado por diseño de este encargo

No se corrió `--run --with-runtime` completo (fuera del alcance del worker); la corrección se valida solo con los dobles de prueba arriba. La próxima corrida completa de `--run --with-runtime` debería confirmar en vivo que el turno de Codex de la evidencia original (attempt-20) ahora se clasifica `unavailable` con motivo `timeout after 900 s` en vez de matar el arnés, y que `inspector.runtime`/`eof_gate`/`composed_prior_positives` quedan poblados aun si Codex expira.
