# W44 — `codex-model-qualifier.py`: un descendiente zombi se cuenta como vivo (flake ~36 % del gate `core` bajo macOS 27.0)

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker de scripts. Orquestador: Claude Opus 5. Sin subagentes, sin segundo plano (Bash `timeout` hasta 600000 ms), sin commit, sin Docker, sin red. **Archivos permitidos (y ningún otro):** `scripts/codex-model-qualifier.py`, `scripts/test-codex-model-qualifier.py`.

## Síntoma

La etapa `codex-qualifier-tests` del gate `core` falla de forma intermitente. Medido por el orquestador sobre bytes **sin modificar** (ambos archivos son byte-idénticos a `7c477db`, donde la etapa pasó 39/39 el 2026-09-15): **5 fallos en 14 ejecuciones**, más el fallo dentro del gate. Afecta a tres tests distintos —`test_fake_end_to_end_source_immutable_and_cleanup` (3×), `test_large_staged_binary_is_excluded_from_fixture_budget`, `test_large_private_and_schema_bundle_entry_inventories`— siempre con `TransportCloseError: <phase>:transport cleanup invalid`, y en 3 de 5 además con `monitor:live descendant executable unresolved:<pid>:ProcessLookupError`.

No es defecto de producto ni regresión de esta rama: es un defecto **latente** del arnés que el salto del host a **macOS 27.0** (y a Xcode completo) ha destapado al cambiar la temporización de salida y cosecha de procesos.

## Causa raíz (ya diagnosticada; verifícala, no la re-descubras)

**Un proceso zombi —salido pero aún no cosechado— se cuenta como vivo.** `os.kill(pid, 0)` tiene éxito sobre un zombi porque el pid sigue ocupando la tabla de procesos, y `proc_pidpath` sobre ese mismo zombi ya falla con `ESRCH`. La contradicción entre ambos hechos es lo que revienta, en dos sitios:

1. **`scripts/codex-model-qualifier.py:439-442`** — `process_executable(pid)` lanza `OSError`/`ProcessLookupError`; el guardia pregunta `process_is_live(pid)`, que devuelve `True` para el zombi, y se lanza `RuntimeError("live descendant executable unresolved:...")`. El `continue` de la línea 442 (la rama benigna «ya no está») nunca se alcanza en la ventana de zombi.
2. **`scripts/codex-model-qualifier.py:483 y 490`** — `remaining_pids` se calcula intersectando `self.observed` con las filas de `self._rows()`, que **incluyen zombis**, tras un `time.sleep(.05)` fijo. Un zombi superviviente a esos 50 ms deja `remaining` no vacío → `require_transport_closed` (línea 342) lanza `TransportCloseError`.

El dato necesario **ya se está leyendo**: `DarwinBsdInfo` (línea 357) tiene el campo `status`, que es `p_stat` de `kinfo_proc`, donde `SZOMB == 5`.

## Qué tienes que hacer

Haz que el arnés distinga **proceso en ejecución** de **zombi**, en los dos sitios, y que un zombi no cuente como descendiente vivo:

- Un discriminante de estado zombi por plataforma: en Darwin, el `status == SZOMB (5)` que ya devuelve `proc_pidinfo(..., PROC_PIDTBSDINFO=3, ...)`; en Linux, el estado `Z` de `/proc/<pid>/stat`. Que `process_is_live` (o el guardia que la usa en 441) deje de tratar un zombi como vivo.
- Que el cálculo de `remaining_pids` excluya zombis, en vez de depender de que `time.sleep(.05)` baste para que los cosechen. El `sleep` fijo es una carrera, no una espera: si mantienes una espera, que sea **acotada y con condición** (esperar a que los pids observados dejen de estar en ejecución, hasta un techo, en vez de dormir a ciegas), pero la corrección no debe depender de la duración.

## La propiedad de seguridad no se debilita — y esto es lo que se te evalúa

El arnés existe para garantizar que **ningún descendiente en ejecución queda sin identificar ni sin matar**. Un zombi no ejecuta código: no puede abrir sockets, ni escribir, ni sobrevivir a la fase. Excluirlo hace la comprobación **más precisa**, no más laxa. Lo que **no** puedes hacer:

- Convertir en benigno un pid que sí está en ejecución y cuyo ejecutable no se resuelve — eso debe seguir siendo un fallo ruidoso.
- Relajar `unexpected descendant executable` (444), `descendant executable identity` (446), `foreign or reused pid` (486/489) ni `required descendant not observed` (492).
- Silenciar `self.failure` ni aflojar `require_transport_closed` (342).
- Ampliar el `sleep` y llamarlo arreglo.

Si al implementarlo concluyes que el diagnóstico es incorrecto o incompleto, **dilo y para**; no fuerces un cambio para que los tests pasen.

## Verificación obligatoria

```sh
python3 -B scripts/test-codex-model-qualifier.py
```

**20 ejecuciones consecutivas, todas OK**, y pega el recuento. Con una tasa base de ~36 %, 20 ejecuciones limpias es la evidencia mínima; menos no distingue el arreglo de la suerte. Si alguna falla, informa de la firma exacta en vez de repetir hasta que salga bien.

Añade además **tests deterministas** del comportamiento nuevo, sin depender de una carrera real: un doble o inyección que presente un pid en estado zombi debe quedar excluido de `remaining_pids` y no disparar «live descendant executable unresolved»; un pid **en ejecución** cuyo ejecutable no se resuelve debe seguir fallando.

Informe (causa → cambio → por qué la propiedad se mantiene, recuento de las 20 ejecuciones y cifras de los tests) en `docs/validation/M8/delegation/W44-qualifier-zombie-descendants/report.md` y en tu última respuesta. No commit.
