# W09d — arnés de clientes M6 robusto a la no-determinación de assists del analyzer

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker sobre el arnés (`scripts/test-m6-clients.py`, `scripts/m6-inspector-session.mjs`, `test-m6-clients-unit.py`). Orquestador: Claude Fable 5.1 (como Opus 4.8). No puedes lanzar subagentes. **Nunca corras un comando en segundo plano. No hagas commit.** Puedes correr `--run` y los unit tests; NO corras `--with-runtime` (lo corre el orquestador con Docker).

## Diagnóstico (vinculante — ya establecido por el orquestador)

En `--with-runtime` (attempt-5): el cliente **Inspector** pasó la matriz completa de los 5 tools incluido el ciclo de escritura apply preview→commit→receipt + los dos negativos (`ACTION_STALE`, `FILE_NOT_IN_SNAPSHOT`). El cliente **Claude Code** hizo discovery (36 tools) + las lecturas + el negativo `FILE_NOT_IN_SNAPSHOT`, pero su llamada `rust.analyzer.actions` (petición idéntica a la de Inspector: file `src/lib.rs`, range `{start:{2,9},end:{2,9}}`) devolvió `actions: []` con `completeness.state=complete`, así que el modelo **correctamente** no pudo aplicar (no hay `action_digest`) y saltó al negativo. Los code actions (assists) de rust-analyzer no están garantizados por el readiness `serverStatus` quiescent en una instancia transitoria por consulta (ADR-084): son intermitentes. El oráculo actual `validate_runtime_model_flow` exige que Claude reabra el proyecto de escritura exactamente dos veces (que haga la escritura), lo que es irrazonable dado (a) la no-determinación de assists y (b) que un cliente dirigido por modelo es best-effort. La prueba autoritativa del camino de escritura vive en el cliente Inspector + los 3 e2e nativos de apply + el corte `m6-11` + la etapa `m6-runtime` del gate (todo verde).

## Cambios

### 1. Inspector: reintento acotado de la captura de `actions` (`m6-inspector-session.mjs` + su plan)

Para la fila runtime de `rust.analyzer.actions` (la que captura el `action_digest` para el apply), envuelve la llamada en un reintento acotado: si `data.actions` viene vacío con `completeness.state=complete`, reintenta la MISMA llamada (idempotente, read-only) hasta N veces (p.ej. 3) con una espera corta (p.ej. 1-2 s) entre intentos, hasta obtener ≥1 acción aplicable con `action_digest`. Si tras N intentos sigue vacío, falla esa fila con un mensaje claro ("analyzer offered no assist after N retries — known assist-readiness race, W09d") en vez de un error opaco. Esto hace la demostración de escritura del Inspector fiable frente a la carrera de readiness. NO cambies las demás filas ni el comportamiento del negativo.

### 2. Claude Code: oráculo runtime robusto a assists vacíos (`validate_runtime_model_flow`)

Reescribe el oráculo para que:
- **Exija** (determinista, siempre): discovery (36 tools), los tres roots abiertos, las lecturas positivas (`symbols` document + workspace, `references`, `diagnostics`) una vez cada una con el resultado planeado, la llamada `actions` una vez, y al menos un negativo (`FILE_NOT_IN_SNAPSHOT`). Ninguna otra capability MCP usada.
- **Camino de escritura best-effort**: si la respuesta de `actions` del modelo trajo ≥1 acción aplicable Y el modelo ejecutó apply preview→commit→receipt + la reapertura + el negativo `ACTION_STALE`, **valida ese ciclo completo** exactamente como antes (reapertura del write project, ref invalidado, receipt committed, write en disco, stale). Si `actions` del modelo vino vacío (`completeness complete`, sin acciones) O el modelo no ejecutó el apply, **no falles**: registra `claude_runtime_write: "skipped: analyzer offered no applicable action"` (detéctalo del transcript: cero `action_digest` capturables) y considera el flujo válido. En ningún caso conviertas un fallo real (una lectura que no dio el status planeado, o el uso de otra capability, o un negativo ausente) en éxito.
- El receipt debe registrar claramente, por cliente, qué se ejecutó: `inspector.write_lifecycle: performed`, y `claude_code.write_lifecycle: performed | "skipped: no applicable action offered"`.

### 3. Nota de deuda

Añade una línea en el docstring del módulo o donde el arnés documente sus límites: la disponibilidad de assists de rust-analyzer es no-determinista bajo el modelo de instancia transitoria; la prueba autoritativa de escritura es Inspector + los e2e nativos; deuda M6-04 = readiness de assists más fuerte o reintento en el gateway.

Mantén el arreglo de fingerprint (W09b) y el de Tasks (W09c). No toques la matriz docker-free (verde).

## Verificación (foreground, sin Docker)

```text
python3 -B scripts/test-m6-clients-unit.py
python3 -B scripts/test-m6-clients.py --run
```

Ambos verdes. Añade unit tests para: (a) el oráculo runtime acepta un transcript de Claude sin escritura cuando `actions` vino vacío; (b) el oráculo runtime SIGUE validando el ciclo de escritura completo cuando el transcript sí lo trae; (c) el oráculo runtime RECHAZA un transcript que omite una lectura o usa otra capability. Reporta: Task / Result / Files changed / La lógica exacta de reintento del Inspector y del oráculo best-effort / Tests (unit + `--run`, con conteos) / Risks / Open issues. No commit.
