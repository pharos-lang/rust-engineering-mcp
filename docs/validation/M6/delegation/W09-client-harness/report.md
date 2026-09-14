# W09 — arnés de clientes stock M6 (`scripts/test-m6-clients.py`)

| Campo | Valor |
| --- | --- |
| Modelo solicitado | Claude Sonnet 5 (`claude -p --model sonnet --effort high --disallowedTools Agent Task`) |
| CLI | `claude` 2.1.267 |
| Inicio / fin (UTC) | 2026-09-12T23:30:36Z / 2026-09-13T00:11:11Z |
| Resultado | `subtype: success`, `is_error: False`, 207 turnos |
| Alcance | Escribir el arnés de cualificación de clientes stock (Inspector + Claude Code) sobre los 5 tools del analyzer, su test unitario y el driver `.mjs`, más el cableado SonarCloud. No commit, sin Docker. |

## Entregables (verificados por el orquestador)

- `scripts/test-m6-clients.py` (1506 líneas) — preflight + `--run` + `--run --with-runtime` + `proxy`, reutilizando `load_m3()`.
- `scripts/test-m6-clients-unit.py` (956 líneas, **91 tests**) — derivación del plan, estrictez del oráculo, chequeo de modelo del transcript de Claude, ensamblado del recibo.
- `scripts/m6-inspector-session.mjs` (272 líneas) — driver Inspector, adaptado al ciclo de escritura; sin oráculo de Resource (estos 5 tools no publican artefacto).
- `.github/workflows/sonarcloud.yml` — `test-m6-clients-unit.py` añadido a la lista de cobertura.
- `sonar-project.properties` — `scripts/m6-inspector-session.mjs` añadido a `sonar.coverage.exclusions` (precedente m3/m4/m5 `.mjs`). `test-m6-clients.py` no necesita entrada (cubierto por el glob `scripts/test-*.py`, igual que `test-m5-clients.py`).

## Verificación del orquestador (Docker-free)

- `python3 -B scripts/test-m6-clients-unit.py` → **91 tests, OK** (re-ejecutado por el orquestador).
- `python3 -B scripts/test-m6-clients.py` (preflight) → `status: ready`, `unsatisfied: []`, inventario **36** (5 M6 apilados, 31 previos `previous_unchanged: true`), imagen `sha256:f39a5b33…`, plan runtime de 10 filas (ciclo preview→commit→receipt, fila `ACTION_STALE`, fila `FILE_NOT_IN_SNAPSHOT`).

## Decisiones del worker (aceptadas)

- **Caso cancel (G4)** limitado al cliente Inspector vía `cancelToolCall()`; `claude -p` no interactivo no puede cancelar una sola llamada MCP en vuelo sin matar el proceso. Razonable.
- `operation_id == plan_id` (verificado en `mutation/analyzer_action.rs`) permite reusar el `plan_id` del preview en commit/receipt.
- Mantuvo `CLAUDE_EFFORT = "medium"` para el cliente Claude Code *embebido* (el que está bajo prueba), distinto del `high` con que se lanzó el worker.
- Retiró código muerto `materialize_apply_arguments` (superado por `_matches_fact`).

## Pendiente para el orquestador

- Ejecutar `--run` (matriz de rechazo Docker-free) y `--run --with-runtime` (matriz real) con Docker → recibo `docs/validation/M6/clients.json`.
- `docs/ci.md` §17-38 (grupo 3 "Clientes reales") no menciona `m6-inspector-session.mjs` — actualización de doc del orquestador/integrador.
