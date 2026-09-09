# Intentos de gate M5, incluidos los fallidos

Un gate que falla es evidencia, no basura. Estos recibos se conservan enteros.

## Intento 1 — 2026-09-09, `full`, **failed** en el paso 30 de 34

Recibos: [gate](full-gate-attempt-1.json) · [m4-runtime](m4-runtime-attempt-1.json).

Corrido sobre un worktree limpio en `b19c3cd`, porque el árbol principal tenía
trabajo de dependencias del owner sin commitear (subidas de `lancedb`,
`fastembed`, `jsonschema` y `tokio-rustls`) y un recibo no debe acreditar bytes
que quien lo firma no controla.

Veintinueve pasos pasaron, entre ellos `docker-security`, `rust-security`,
`m2-runtime`, `m3-runtime` y los cuatro pasos M5 de helper y vendor. Falló
`m4-runtime` en la última de sus catorce selecciones,
`security_runtime::deny_native_mcp_tasks_policy_licenses_and_owner_bound_redacted_resource`,
con `Error: Timeout` a los 28,1 s y exit 101.

### Condición de diagnóstico, fijada antes de correrlo

Se escribe aquí **antes** de reproducir, para que la clasificación no se elija
después de ver el resultado:

- El recibo calificado de M4 registra esa misma selección en **42,5 s y
  `passed`** ([M4-runtime.json](../M4-runtime.json)). El fallo llegó a los 28,1 s,
  es decir **antes** de lo que tarda normalmente: abortó contra un deadline
  interno del propio test, no contra el bound de 900 s del gate.
- El `load` de la máquina era **15,97 en 16 núcleos**, con dos agentes
  compilando en paralelo al gate. Eso incumple la regla operativa que este mismo
  proyecto ya tenía registrada tras dos flakes de la misma clase hoy —el de
  `closed_stdout_exits_even_when_stdin_remains_open` y el de
  `test-codex-model-qualifier.py`—, y el incumplimiento fue del owner, no del
  producto.

**Criterio.** Se corre esa selección **sola**, con `load` por debajo de 6:

- si **pasa**, se clasifica como flake de planificación, se registra la
  disposición y **no** se toca el test ni se amplía su timeout: ampliarlo
  convertiría un arnés que detecta cuelgues reales en uno que no;
- si **falla igual**, no es carga. Es un defecto y se persigue como tal, sin
  relanzar buscando un verde.

Nada de este intento acredita bytes finales en ningún caso: ADR-079 y ADR-080
cambian contratos que este gate midió, así que la pasada que cuenta es posterior.
