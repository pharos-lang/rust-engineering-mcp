# W09e — pacing entre lotes de cliente para el workload pesado de contenedores del analyzer

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker sobre `scripts/test-m6-clients.py` (+unit). Orquestador: Claude Fable 5.1 (como Opus 4.8). No puedes lanzar subagentes. **Nunca corras en segundo plano. No hagas commit. No corras `--with-runtime`** (lo corre el orquestador con Docker); sí puedes correr `--run` y los unit tests.

## Diagnóstico (vinculante — establecido por el orquestador)

En `--with-runtime`, el cliente **Inspector** pasa la matriz completa de los 5 tools de forma fiable (3/3 corridas), incluido el ciclo de escritura apply preview→commit→receipt + los negativos. El cliente **Claude Code** corre DESPUÉS del Inspector en la misma invocación; tras los ~10 spawns de contenedores rust-analyzer (imagen de 2.5 GB) del Inspector, el host/Docker satura y algunos spawns del lote de Claude fallan transitoriamente → `unavailable/SANDBOX_DENIED` ("failed calibration or current capacity"). Patrón observado en el lote de Claude: symbols(document) **passed**, luego symbols(workspace)/references/diagnostics **SANDBOX_DENIED** (ráfaga de ~segundos), luego actions **passed** de nuevo. Es una carrera de capacidad transitoria del host, no un defecto de M6: el spawn del contenedor falla brevemente y se rechaza honestamente.

## Cambio: pacing entre lotes

Inserta, **entre el gate runtime del Inspector y el gate runtime de Claude Code** (y solo en modo `--with-runtime`), un asentamiento acotado de Docker para que el lote de Claude empiece con capacidad fresca:
1. Espera a que no queden contenedores rust-analyzer del lote anterior (consulta `docker ps` filtrando la imagen M6 / el prefijo de nombres que usa el gateway, con un timeout acotado — reutiliza cualquier helper de M3/M5 si existe; si no, un bucle de sondeo con `subprocess` y un tope, p.ej. 60 s).
2. Una pausa fija breve adicional (p.ej. `COOLDOWN_SECONDS = 20`) para que el host recupere memoria antes del lote de Claude.
Documenta ambos con constantes nombradas. NO metas la pausa en el modo docker-free ni entre filas dentro de un lote. Mantén todo lo demás (fingerprint W09b, Tasks W09c, robustez de assists W09d) intacto.

Si el arnés ya tiene un punto claro donde separa "Inspector runtime" de "Claude runtime" en `run()` (alrededor de las llamadas a `inspector_gate(..., RUNTIME, ...)` y `claude_gate(..., RUNTIME, ...)`), pon el asentamiento justo antes del `claude_gate` runtime.

## Verificación (foreground, sin Docker real)

```text
python3 -B scripts/test-m6-clients-unit.py
python3 -B scripts/test-m6-clients.py --run
```

Ambos verdes (el `--run` docker-free no ejerce el cooldown, que es solo `--with-runtime`; confirma que no rompiste `--run`). Añade/actualiza un unit test que verifique que el asentamiento se invoca en el camino runtime y NO en docker-free (mockea el helper de Docker/espera). Reporta: Task / Result / Files changed / Dónde y cómo se asienta Docker (constantes, timeout) / Tests / Risks / Open issues. No commit.
