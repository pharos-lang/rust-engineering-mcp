# W09f — oráculo del cliente dirigido por modelo tolerante al rechazo transitorio de capacidad del analyzer

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker sobre `scripts/test-m6-clients.py` (+unit). Orquestador: Claude Fable 5.1 (como Opus 4.8). No puedes lanzar subagentes. **Nunca corras en segundo plano. No hagas commit. No corras `--with-runtime`.** Puedes correr `--run` y los unit tests.

## Diagnóstico (vinculante — establecido y reproducido por el orquestador, 4 corridas)

- El cliente **Inspector** (determinista, Node) pasa la matriz completa de los 5 tools + ciclo de escritura + negativos de forma fiable (4/4).
- El cliente **Claude Code** (dirigido por modelo) golpea un rechazo TRANSITORIO de capacidad: una llamada de analyzer exitosa tarda 2-13 s (spawn del contenedor de 2.5 GB + indexado); la capacidad acotada del analyzer (presupuestos ADR-084) se **libera de forma asíncrona** tras devolver la respuesta, así que una llamada disparada inmediatamente después recibe un `unavailable/SANDBOX_DENIED` **instantáneo** (`duration_ms: 0`) hasta que termina el teardown de la sesión previa. Patrón reproducible: `symbols(document)` **passed** (~13 s, arranque en frío), luego `symbols(workspace)`/`references`/`diagnostics` **SANDBOX_DENIED instantáneo**, luego `actions` **passed** de nuevo. Inspector no lo golpea porque su cliente espacia lo suficiente. NO es un defecto de corrección: los tools funcionan (gate `full` verde + Inspector).
- G4 exige del cliente dirigido por modelo: discovery → llamada positiva → fallo. Claude lo cumple de forma fiable: discovery (36 tools) + `symbols(document)` positivo + `actions` positivo + el negativo `FILE_NOT_IN_SNAPSHOT`. La cobertura exhaustiva por-tool y la escritura son autoritativas del Inspector + los e2e nativos.

## Cambio: `validate_runtime_model_flow` tolerante a capacidad transitoria

Reescribe el oráculo runtime de Claude para que:

- **Exija siempre (hard)**: no usar otra capability MCP; los 3 `project.open`; **al menos una** lectura de analyzer positiva (`symbols` document es la primera y arranca en frío, pasa de forma fiable); la llamada `actions` servida (positiva con acciones, o vacía-completa por [[W09d]], ambas válidas); el negativo `FILE_NOT_IN_SNAPSHOT` con su `blocked`. Cada fila servida positivamente debe traer EXACTAMENTE el status/error_code planeado.
- **Tolere (no falla)**: un `unavailable/SANDBOX_DENIED` en una lectura de analyzer (`symbols` workspace, `references`, `diagnostics`) como **rechazo transitorio de capacidad** — regístralo como `capacity_refused` en el receipt (por fila), NO cuenta como positivo, NO es fallo. Para evitar confundirlo con un rechazo real, acéptalo solo cuando el status sea `unavailable` y el error_code `SANDBOX_DENIED` (el código honesto de capacidad); cualquier OTRO status/código inesperado en una fila positiva SIGUE siendo fallo duro.
- **Rechace (hard fail)**: `symbols(document)` (la primera positiva) no positiva; `actions` ausente o con status inesperado; el negativo ausente o con código distinto de `FILE_NOT_IN_SNAPSHOT`; cualquier otra capability MCP; una respuesta con un status que no sea ni el positivo planeado ni `unavailable/SANDBOX_DENIED` ni el negativo planeado.
- **Escritura best-effort** (conserva [[W09d]]): si con capacidad Claude obtuvo acciones y ejecutó el ciclo, valídalo; si no, `write_lifecycle: skipped`.
- El receipt debe registrar por cliente y fila lo observado: `positive | capacity_refused | negative`, y por cliente `analyzer_positive_reads` (conteo) y `write_lifecycle`. Deja claro en el receipt que Inspector es la matriz autoritativa.

Añade un comentario en el módulo documentando el mecanismo (liberación asíncrona de capacidad; cliente rápido dirigido por modelo ve `SANDBOX_DENIED` transitorio; Inspector espacia y cubre la matriz exhaustiva) y la **deuda M6**: `SANDBOX_DENIED` mezcla el rechazo PERMANENTE (sin grant `--rust`) con el TRANSITORIO (capacidad); considerar un código retryable distinto (p.ej. `CAPACITY`/`LOCK_BUSY`) para que un cliente sepa reintentar.

No toques la matriz docker-free, el fingerprint (W09b), Tasks (W09c), ni el pacing (W09e). El modo docker-free sigue exigiendo `SANDBOX_DENIED` en las cinco filas (ahí es el rechazo permanente por socket muerto, no capacidad — sigue siendo hard-required).

## Verificación (foreground)

```text
python3 -B scripts/test-m6-clients-unit.py
python3 -B scripts/test-m6-clients.py --run
```

Ambos verdes. Añade unit tests: (a) el oráculo runtime acepta un transcript con `symbols(document)`+`actions` positivos, `workspace`/`references`/`diagnostics` en `unavailable/SANDBOX_DENIED`, y el negativo — marcando `capacity_refused`; (b) RECHAZA si `symbols(document)` (la primera positiva) viene `unavailable`; (c) RECHAZA una fila positiva con un status inesperado que NO sea `SANDBOX_DENIED` (p.ej. `failed`); (d) sigue validando el ciclo de escritura completo cuando está presente. Reporta: Task / Result / Files changed / La lógica exacta (qué es hard vs tolerado) / Tests / Risks / Open issues. No commit.
