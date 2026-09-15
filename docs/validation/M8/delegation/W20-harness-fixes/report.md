# W20 — correcciones de arneses: soak reutiliza `project_ref`; pin de Gemini CLI 1.2.2

## Task

1. `scripts/soak-m8.py`: la calibración de 50 ciclos falló `fd_growth` (8 → 57)
   porque el ciclo llamaba `rust.project.open` en cada iteración, contradiciendo
   la decisión de `docs/validation/M8/05.md` de reutilizar un único
   `project_ref`. Cambiar el ciclo para abrir el proyecto una sola vez, medir
   `fd_growth` solo sobre esa fase principal, y mover el churn de aperturas a
   una fase opcional separada (evidencia, no criterio de fallo).
2. `scripts/test-m8-clients.py`: el preflight bloqueaba por `gemini_cli 1.2.1 ->
   1.2.2` (versión real instalada). Fijar el pin a `agy` 1.2.2 y exponer
   `pinned_versions` en el recibo de preflight.
3. Dejar constancia en `docs/validation/M8/05.md` §Soak.

## Result

Ambos arneses corregidos, ejecutados de verdad contra el binario y el `agy`
instalados, y verdes. `python3 -B scripts/soak-m8.py --profile core --cycles 30
--hours 0.05 --sample-every 10 --out target/m8-soak-calibration-2.json` quedó
`PASSED`; `python3 -B scripts/test-m8-clients.py --preflight` quedó
`"status": "ready"`, `"unsatisfied": []`.

## Files changed

- `scripts/soak-m8.py`:
  - `run_cycle` ya no llama a `rust.project.open`; el ciclo principal es
    `rust.catalog.status -> rust.crate.search`.
  - Nuevas funciones `open_project()` (abre una vez, valida `status: passed`,
    devuelve `project_ref`) y `run_open_churn()` (fase opcional
    `--open-churn N`, por defecto 20, ejecutada **después** de evaluar los
    criterios; mide FDs antes/después y, si `--ttl-wait-seconds > 0`, también
    tras esperar).
  - `run_core_soak()`: abre el proyecto una vez al inicio; `evaluate_criteria`
    corre solo sobre las muestras de la fase principal (sin churn); añade al
    recibo `project_ref_reopens` (contador honesto, 0 en la práctica porque
    `rust.catalog.status`/`rust.crate.search` no consumen `project_ref` — ver
    nota abajo), `open_churn` (bloque de evidencia) y `notes[]` explicando por
    qué el churn queda fuera del criterio `fd_growth`.
  - Nuevos flags `--open-churn` (default 20) y `--ttl-wait-seconds` (default 0).
  - Docstring del módulo actualizado con la causa raíz de la calibración 1.
- `scripts/test-m8-performance-unit.py`: nuevas clases `OpenProjectTests` y
  `OpenChurnTests` (con un `FakeServer` doble) que cubren `open_project()`
  (extracción de `project_ref`, error si `status != passed`) y
  `run_open_churn()` (conteo de llamadas, muestreo de FDs antes/después, y con
  `--ttl-wait-seconds`).
- `scripts/test-m8-clients.py`: docstring, `AGY_VERSION` `1.2.1 -> 1.2.2`;
  nuevo campo `pinned_versions` en el recibo de `preflight()` con los cuatro
  clientes (`inspector`, `codex`, `claude_code`, `gemini_cli`).
- `docs/validation/M8/05.md`: nueva línea en §Soak documentando la causa de la
  falla de calibración 1 y el cambio de diseño.

## Salidas de soak y preflight

Soak (`target/m8-soak-calibration-2.json`, no comiteado):

```json
"status": "passed",
"project_ref_reopens": 0,
"criteria": {
  "rss_growth": {"plateau_mib": 28.296875, "final_mib": 14.859375, "limit_mib": 33.95625, "passed": true},
  "fd_growth": {"plateau_fds": 8, "final_fds": 8, "limit_fds": 18, "passed": true},
  "state_root_orphans": {"applicable": false, "passed": true},
  "cycle_overrun": {"limit_ms": 150, "count": 0, "passed": true}
},
"open_churn": {"count": 20, "ttl_wait_seconds": 0.0, "fd_count_before": 8, "fd_count_after": 28, "fd_count_after_ttl_wait": null}
```

`fd_growth` pasa de fallar (8 → 57 en la calibración 1) a pasar (8 → 8); el
churn deliberado de 20 aperturas extra al final muestra el crecimiento
esperado de FDs (8 → 28), fuera del criterio.

Preflight (`python3 -B scripts/test-m8-clients.py --preflight`):

```json
"status": "ready",
"unsatisfied": [],
"clients": {"gemini_cli": {"expected": "1.2.2", "observed": "1.2.2", ...}},
"pinned_versions": {"inspector": "2.5.0", "codex": "codex-cli 0.154.0", "claude_code": "2.1.268 (Claude Code)", "gemini_cli": "1.2.2"}
```

## Tests

- `python3 -B scripts/test-m8-performance-unit.py` → 36 tests, OK (32
  preexistentes + 4 nuevos: `OpenProjectTests` × 2, `OpenChurnTests` × 2).
- `python3 -B scripts/test-m8-clients-unit.py` → 61 tests, OK (sin cambios;
  no referenciaba `AGY_VERSION`/`1.2.1`).

## Risks

- `project_ref_reopens` queda estructuralmente en 0 en todas las corridas de
  este arnés: `rust.catalog.status`/`rust.crate.search` (los dos únicos tools
  del ciclo principal) no consumen `project_ref` (docs/tools.md), así que el
  camino de reapertura nunca se ejercita en la práctica dentro de la fase
  principal; queda documentado como tal en `notes[]`, no oculto.
- La corrida real usó 30 ciclos / 0.05 h (calibración corta), no la corrida
  completa de 1000 ciclos / 8 h de M8-09; el `fd_growth` a esa escala aún debe
  confirmarse en el gate real.

## Open issues

Ninguno para este alcance. Sin commit.
