# W09b — corregir el oráculo docker-free de `actions`/`action.apply` en el arnés de clientes M6

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker de implementación sobre el arnés que escribiste en W09 (`scripts/test-m6-clients.py`, `scripts/m6-inspector-session.mjs`). Orquestador: Claude Fable 5.1 (como Opus 4.8). No puedes lanzar subagentes. **Nunca corras un comando en segundo plano. No hagas commit.** Puedes correr `test-m6-clients.py --run` (Docker-free, socket muerto — sí lo usa) y los unit tests; NO corras `--with-runtime` (lo corre el orquestador con Docker real).

## Defecto (diagnosticado y reproducido por el orquestador)

El modo docker-free configura `--rust` con imagen real pero un **socket que no existe**, así que cada llamada que alcanza el runtime se rechaza `unavailable/SANDBOX_DENIED`. Las tres tools de lectura lo hacen. Pero las filas docker-free de `rust.analyzer.actions` y `rust.analyzer.action.apply` pasan `PLACEHOLDER_FINGERPRINT` (`sha256:000…`), y `expected_project_fingerprint` es **obligatorio** (ADR-083 §2): un desajuste es `blocked/CONFLICT`, que el servidor comprueba **antes** de llegar al runtime. El comentario de W09 ("the placeholder fingerprint is never reached because no capture is ever taken") es falso: la comprobación de fingerprint es host-side y va primero.

Reproducción del orquestador (binario release, socket muerto, fixture `analyzer-actions`):
- `rust.analyzer.actions` con el fingerprint **real** del `project.open` → `unavailable/SANDBOX_DENIED`.
- `rust.analyzer.actions` con `PLACEHOLDER_FINGERPRINT` → `blocked/CONFLICT`.

Por eso la sesión Inspector docker-free aborta: `rust.analyzer.actions docker_free positive status blocked != unavailable`.

## Cambio

Las filas docker-free de `rust.analyzer.actions` y `rust.analyzer.action.apply` deben usar el **fingerprint real** capturado del `rust.project.open` de la sesión docker-free (igual que ya haces en las filas runtime), no `PLACEHOLDER_FINGERPRINT`, para que el rechazo sea el uniforme `unavailable/SANDBOX_DENIED` (la propiedad de seguridad que la matriz docker-free quiere demostrar: sin runtime/grant → SANDBOX_DENIED, también para el camino de escritura). El `action_digest` de `apply` puede seguir siendo `PLACEHOLDER_DIGEST` (nunca se alcanza: con el fingerprint correcto, el preview intenta resolver el candidato por el runtime, el socket está muerto y devuelve `SANDBOX_DENIED` antes de mirar el digest).

Aplica el cambio en **ambos** drivers y en el plan:
- El plan/oráculo (`call_plan()` y su validación) para esas dos filas: `expect_status: unavailable`, `expect_error_code: SANDBOX_DENIED` (sin cambio de expectativa), pero el `expected_project_fingerprint` de la llamada ya no es el placeholder sino el real de la sesión.
- `m6-inspector-session.mjs`: al construir la llamada docker-free de actions/apply, sustituye el fingerprint por el capturado del `project.open` de esa sesión (mismo mecanismo que usas en runtime).
- El flujo Claude Code docker-free (`claude_prompt`/oráculo): asegúrate de que el modelo use el fingerprint del `project.open` para actions/apply, o que el oráculo lo tolere; NO exijas el placeholder.
- Si `PLACEHOLDER_FINGERPRINT` queda sin uso tras el cambio, retíralo; conserva `PLACEHOLDER_DIGEST`.
- Actualiza cualquier unit test de `test-m6-clients-unit.py` que fijara el placeholder para esas filas.

Mantén intactas las tres filas de lectura (ya correctas) y toda la matriz runtime.

## Verificación (foreground)

```text
python3 -B scripts/test-m6-clients-unit.py
python3 -B scripts/test-m6-clients.py --run
```

`--run` debe terminar en verde con las cinco filas docker-free en `unavailable/SANDBOX_DENIED` para Inspector y Claude Code, y el socket privado asertado ausente al final. Pega la salida. Reporta: Task / Result / Files changed / Cómo se captura y sustituye el fingerprint real en docker-free (los dos drivers) / Tests (unit + `--run`) / Risks / Open issues. No commit.
