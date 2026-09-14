# W09c — alinear el arnés de clientes M6 con el server real: Tasks advertisement

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker sobre el arnés (`scripts/test-m6-clients.py`; unit test). Orquestador: Claude Fable 5.1 (como Opus 4.8). No puedes lanzar subagentes. **Nunca corras un comando en segundo plano. No hagas commit.** Puedes correr `test-m6-clients.py --run` (Docker-free, socket muerto — solo necesita el binario `docker`, no el daemon) y los unit tests; NO corras `--with-runtime`.

## Defecto (diagnosticado por el orquestador)

Tras el arreglo de fingerprint de W09b, `--run` falla más adelante en `validate_protocol_metadata` → `m3.protocol_summary(path, False)` con `unexpected modern Tasks advertisement for inspector`. Causa: el server stock (los 36 tools, no un subconjunto del analyzer) **anuncia la capability Tasks** (`tasks_advertised: true`) porque otras tools (coverage/mutation/benchmark) la usan; los cinco tools del analyzer NO declaran Tasks a nivel de tool (`tasks_declared: false`), pero la capability a nivel de protocolo está anunciada. W09 solo corrió preflight y fijó (mal) `tasks: false` / `protocol_summary(path, False)`.

**Precedente vinculante — el arnés M5 (`scripts/test-m5-clients.py`), que conduce el mismo server stock:**
- L904: `summary = m3.protocol_summary(path, True)` (Tasks **sí** anunciado).
- L730/733 `client_versions()`: inspector `"tasks": True, "resource": True`; claude_code `"tasks": False, "resource": True`.

## Cambio

Alinea el arnés M6 con el M5 para la negociación a nivel de protocolo, **conservando** la decisión correcta de W09 de que los cinco tools del analyzer no llevan superficie Tasks/Resource propia:
- `validate_protocol_metadata` (L666): `m3.protocol_summary(path, True)` — el server anuncia Tasks.
- `client_versions()` (L525): inspector `"tasks": True`; claude_code `"tasks": False`. Para `resource`: sigue el precedente M5 (`True`) **solo si** el oráculo a nivel de protocolo lo requiere; si `resource: True` fuerza al arnés a esperar que algún tool del analyzer publique un Resource (no lo hacen), déjalo en `False` y documenta en una línea por qué M6 difiere de M5 aquí (los 5 tools no publican artefacto). Decide con base en lo que `m3.protocol_summary`/el oráculo realmente comprueban; explica tu elección en el reporte.
- Ajusta cualquier aserción `tasks_declared`/`tasks_advertised` del oráculo por-fila para que: `tasks_advertised` (protocolo) = true; `tasks_declared` (por-tool del analyzer) = false. Ninguna llamada del analyzer debe declarar Tasks.
- Actualiza los unit tests afectados en `test-m6-clients-unit.py`.

No toques el arreglo de fingerprint de W09b ni la matriz runtime. Si al arreglar Tasks `--run` destapa **otro** defecto pre-existente de W09, corrígelo si es del mismo tipo (una suposición estática equivocada del protocolo/inventario que M5 ya resuelve); si es más profundo o ambiguo, párate y repórtalo en Open issues con el error exacto en vez de adivinar.

## Verificación (foreground)

```text
python3 -B scripts/test-m6-clients-unit.py
python3 -B scripts/test-m6-clients.py --run
```

`--run` debe terminar **en verde**: las cinco filas docker-free en `unavailable/SANDBOX_DENIED` para Inspector y Claude Code, `docker_free_socket_created: false`, socket privado ausente, y `validate_protocol_metadata` OK con Tasks anunciado. Pega la salida completa del final de `--run` (el veredicto/JSON) y de los unit tests. Reporta: Task / Result / Files changed / La decisión sobre `resource` y por qué / Tests / Risks / Open issues (si `--run` sigue rojo, el error exacto). No commit.
