# Intentos de la matriz de clientes M5

Cada intento se conserva entero, pase o falle. El script publica
`docs/validation/M5-clients.json` solo cuando la matriz completa pasa; el recibo
anterior está archivado en [m5-closure-history](../m5-closure-history/inventory.json).

## attempt-1 — 2026-09-09, `passed`, superado

Midió el binario y los contratos anteriores a ADR-079 y ADR-080. Su recibo
sigue en [attempt-1/receipt.json](attempt-1/receipt.json) y no acredita el
candidato de cierre: los contratos de bloat y de logs cambiaron después. En
runtime dejó `model_turn_completed = false`, que es la brecha que el harness
actual cierra con el turno dirigido por modelo.

## attempt-2 — 2026-09-10, `failed`: cuota de Codex agotada

Candidato: HEAD `a88ce53d81006ce011f7fe7a50b8288b53c013a7` en el worktree
limpio `/private/tmp/rust-mcp-m5-closure`; código idéntico a `a2464c4`; servidor
`target/release/rust-engineering-mcp` con SHA-256
`9aaa85f046d02a9b1cd4ab0e307e2115bdefd517ca2f6aa75344896f7f2394f1`. Comando:

```text
RUST_MCP_TEST_SOCKET=/Users/cburgosro/.docker/run/docker.sock python3 -B scripts/test-m5-clients.py --run --with-runtime
```

Exit 1. Traza del harness en [harness-stderr.txt](attempt-2/harness-stderr.txt);
recibo parcial en [attempt-2/receipt.json](attempt-2/receipt.json).

Lo que sí se observó antes del fallo, todo en modo docker-free y sin crear
ningún contenedor:

- Inspector 2.5.0: ocho llamadas planificadas, cada una con el estado y el
  `error_code` declarados por las fuentes del servidor: `rust.benchmark.run`
  `unavailable`/`MISSING_OFFLINE_DATA` y `blocked`/`TASKS_REQUIRED`;
  `rust.benchmark.compare` `blocked`/`ARTIFACT_NOT_FOUND` ×2;
  `rust.profile.flamegraph` `blocked`/`PROFILING_NOT_AUTHORIZED` y
  `blocked`/`TASKS_REQUIRED`; `rust.binary.bloat` `unavailable`/`MISSING_OFFLINE_DATA`
  y `blocked`/`TASKS_REQUIRED`. Resource inexistente rechazada.
- Codex app-server 0.153.0, filas scriptadas: doce `tools/call` y una
  `resources/read` en [protocol.jsonl](attempt-2/protocol.jsonl), validadas fila a
  fila (estado, `error_code` e `isError` preservados) antes del turno de modelo.
- Turno dirigido por modelo (docker-free, `gpt-5.6-sol`, medium): el modelo
  ejecutó dos `custom_tool_call` de enumeración de tools y el app-server cerró
  el turno con `codexErrorInfo: usageLimitExceeded`, `willRetry: false`.
  `account/rateLimits/updated` registra `usedPercent: 100` sobre una ventana
  de 10080 min, créditos `0` y `resetsAt: 1789435413`
  (2026-09-15T01:23:33Z; 2026-09-14 20:23 hora local). Sin ningún
  `mcpToolCall`, el oráculo `set(M5_TOOLS) ⊆ observed` falla y el harness
  aborta antes del modo runtime. Eventos en
  [codex-docker_free-model-events.jsonl](attempt-2/codex-docker_free-model-events.jsonl).

Comprobado después: `docker ps -a` y `docker volume ls` sin contenedores ni
volúmenes propios; directorio privado del harness eliminado
(`private_directory_removed = true`); `evidence_credential_scan = clean`.

**Disposición.** Bloqueo ambiental, no del producto ni del oráculo. No se
debilita la exigencia del turno dirigido por modelo, no se cambia el modelo ni
se copian credenciales. La matriz se repite, sin cambiar código, cuando la
cuota se restablezca. Mientras tanto M5-05 no puede declararse Done y M5 sigue
`In progress` aunque los gates nativo, `core` y `full` acrediten el candidato.

## attempt-3 — 2026-09-10, `failed`: falso positivo del escaneo de credenciales del harness

Primer intento con Claude Code como cliente agentic (HEAD `c3cb12a`). Inspector
docker-free pasó sus ocho filas y el turno docker-free de Claude se completó
(`claude-docker_free-model-events.jsonl`), pero el escaneo de texto con forma de
credencial que la revisión Sonnet pidió añadir marcó la palabra `authorization`
dentro de la prosa final del modelo («per host/authorization/vendor-data
policy»). El patrón heredado del escaneo de metadatos de protocolo era
vocabulario, no forma; se restringe a `authorization:` (cabecera) y a los
prefijos de token. Ningún contenedor se creó; sin residuo Docker.

## attempt-4 — 2026-09-10, `failed`: timeout por llamada del Inspector

HEAD `09ddae7`. Docker-free completo: Inspector ocho filas y turno Claude con
los cuatro rechazos ligados a sus roots (17,7 s). En runtime, la primera fila
`rust.benchmark.run` (presupuesto 300 s) recibió `REQUEST_TIMEOUT` a los 61 s:
el driver pasaba `request_timeout_ms` como `serverSettings.requestTimeout`, una
entrada de configuración que el cliente no consulta, y el SDK aplicó su
`DEFAULT_REQUEST_TIMEOUT_MSEC` de 60 s. Ninguna fila anterior había superado
ese umbral. El Inspector envió `notifications/cancelled`, el servidor canceló
la medición y no quedó artifact ni contenedor: comportamiento correcto del
producto. Se corrige el driver (`timeout: plan.request_timeout_ms`).

## attempt-5 — 2026-09-10, `failed`: expectativa de plan contraria al contrato congelado

HEAD `b20a4c6`. Docker-free completo (Inspector y Claude). En runtime pasaron
las dos mediciones, la comparación, el profiling, el bloat y la primera fila de
recuperación de logs (fallo de compilación observado); la sesión duró 227,8 s.
Falló la última fila, harness no reconocido: el plan esperaba
`observation.harness == "unrecognized"` y el contrato congelado
(`benchmark-run-tool.json`, `Harness` con `serde(tag = "harness")`) publica el
objeto `{"harness": "unrecognized"}`. Esa fila nunca se había ejecutado contra el
servidor —el plan de attempt-1 no la tenía— y era el P3 que la revisión Codex de
bloat señaló. Se corrige solo la expectativa y se liga al snapshot: cualquier
`expect_observation.harness` debe ser una variante del `oneOf` de `Harness`,
con test que rechaza el string. Sin residuo Docker.

## attempt-6 — 2026-09-10, `passed`

HEAD `dc7ce3e`, servidor `9aaa85f0…`. Recibo publicado en
[M5-clients.json](../M5-clients.json). Inspector 2.5.0: quince filas —ocho
docker-free y siete runtime— con catorce Resources leídas en runtime (dos
mediciones Criterion de una ejecución, comparación `inconclusive` con
`insufficient_executions`, profiling con 195 muestras y 0 perdidas, bloat
validado con cargo-bloat 0.12.1, y las dos filas de recuperación de logs de
ADR-080 §6). Claude Code 2.1.267 (`claude-sonnet-5`): turno docker-free con los
cuatro rechazos declarados ligados a sus roots (15,8 s) y turno runtime con
siete llamadas exactas —open, discovery, dos mediciones propias, comparación
positiva `inconclusive`/`insufficient_executions`, `NOT_A_DATASET` con su
`criterion_archive` y lectura nativa de esa Resource, cuyos 40960 bytes hashean
al `sha256` publicado (174,7 s)—. Ningún mensaje `assistant` de otro modelo;
`permission_denials` vacío; el único archivo que el cliente dejó bajo su home
se contó y eliminó. Sin contenedores ni volúmenes residuales.

## attempt-7 — 2026-09-10, `passed` (lock con `lancedb 0.31.0`, opción 2a)

HEAD `a19d741`, servidor `2c396063…` reconstruido sobre el lock revertido. Mismo
resultado que attempt-6 sobre los nuevos bytes: Inspector 2.5.0 quince filas y
catorce Resources en runtime (profiling 193 muestras, 0 perdidas; bloat
validado); Claude Code 2.1.267 (`claude-sonnet-5`) con los dos turnos dirigidos
por modelo —docker-free 17,7 s; runtime 184,8 s con dos mediciones propias,
comparación `inconclusive`/`insufficient_executions`, `NOT_A_DATASET` y lectura
de su `criterion_archive` con 40960 bytes que hashean al artifact publicado—.
Sin residuo Docker; escaneo de credenciales limpio. Este recibo sustituye a
attempt-6, que queda como historia del lock 0.38.0. Un arranque previo sobre
`37805f6` se abortó en su primer minuto porque el worktree aún contenía el
`M5-clients.json` anterior; no midió nada y no dejó residuo.
