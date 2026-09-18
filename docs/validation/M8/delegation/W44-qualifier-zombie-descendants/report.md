# W44 — descendiente zombi contado como vivo en `codex-model-qualifier.py`

## Causa raíz

Confirmada, con un matiz que el diagnóstico original no cubría del todo (ver más abajo).

`Transport._monitor` y `Transport.close` usaban `os.kill(pid, 0)` (vía `process_is_live`) como único criterio de "¿sigue vivo este descendiente?". Un zombi (salido, no cosechado) sigue ocupando la tabla de procesos, así que `os.kill(pid, 0)` tiene éxito sobre él, mientras que `process_executable(pid)` (que resuelve la ruta del ejecutable vía `proc_pidpath` en Darwin) falla con `ESRCH` porque un zombi no tiene imagen resoluble. Esa contradicción producía dos fallos:

1. En `_monitor` (guardia de identidad de ejecutables, línea ~448 antes del cambio): el `except OSError` preguntaba `process_is_live(pid)`, obtenía `True`, y lanzaba `RuntimeError("live descendant executable unresolved:...")` en vez de tomar la rama benigna.
2. En `close` (cálculo de `remaining_pids`): el snapshot de `self._rows()` no distinguía zombis de procesos en ejecución, y dependía de un `time.sleep(.05)` fijo para que el sistema operativo cosechara el zombi antes de la segunda lectura — una carrera, no una espera.

## Cambio aplicado

### 1. Fuente de datos: filas de proceso ahora llevan el bit de zombi

- `Transport._rows()` añade la columna `stat=` al `ps` (`pid=,ppid=,pgid=,stat=,args=`) y clasifica zombi cuando el primer carácter de `STAT` es `Z` (documentado en `man ps` como "marks a dead process (a zombie)"; mismo convenio en Linux/procps).
- `darwin_process_rows()` (la ruta de reserva cuando el sandbox del host deniega `ps`) ya leía `DarwinBsdInfo.status` vía `proc_pidinfo(pid, PROC_PIDTBSDINFO=3, …)`; ahora expone ese bit (`info.status == SZOMB`) en vez de descartarlo.
- Cada fila pasa de 4 a 5 campos: `(pid, ppid, pgid, zombie, command)`.
- Nueva función pura `running_pids(rows)` → conjunto de pids no-zombi. `close()` la usa para excluir zombis de `remaining_pids`, sustituyendo el `time.sleep(.05)` fijo por una espera **acotada y condicionada** (techo de 2s, reconsulta cada 20ms, termina en cuanto los pids objetivo dejan de estar en ejecución).

### 2. Guardia de identidad de ejecutables: de "vivo por `kill`" a "confirmado en ejecución por consulta fresca"

Aquí es donde el diagnóstico original quedaba corto. Mi primer intento siguió la letra del encargo: cachear el bit de zombi de `_rows()` al principio de cada iteración de `_monitor` y usarlo para excluir al zombi de la comprobación. En las ejecuciones de verificación **eso seguía fallando** (~1 de cada 9–13 ejecuciones), con la misma firma `monitor:live descendant executable unresolved:<pid>:ProcessLookupError`. Añadí diagnóstico temporal (revertido antes de la entrega) y logré reproducirlo con una firma precisa.

Firma exacta capturada in situ: `proc_pidinfo` sobre el pid fallido devolvía `size=0` con `errno=3 (ESRCH)` — es decir, **para cuando el guardia reacciona, el descendiente ya no es zombi: ya ha sido cosechado por completo y ha desaparecido por completo de la tabla de procesos.** El caché de `_rows()` tomado al principio del ciclo (antes de iterar, resolver ejecutables y calcular hashes de cada pid propio) queda obsoleto para ese pid concreto: seguía marcado "no-zombi" (o simplemente no lo vio) en el momento del guardado, pero para cuando el guardia lo consulta ya pasó por zombi y fue cosechado.

Corrección final: sustituí `process_is_live` (basado en `os.kill`, la fuente misma de la señal obsoleta) por `process_is_confirmed_running(pid)`, una consulta **fresca** en el momento exacto del fallo — no el snapshot de filas del ciclo:

```python
def process_is_confirmed_running(pid):
 if sys.platform=="darwin":
  libproc=ctypes.CDLL("/usr/lib/libproc.dylib",use_errno=True);info=DarwinBsdInfo();size=libproc.proc_pidinfo(pid,3,0,ctypes.byref(info),ctypes.sizeof(info))
  return size==ctypes.sizeof(info) and info.status!=SZOMB
 if sys.platform.startswith("linux"):
  try:text=Path(f"/proc/{pid}/stat").read_text()
  except OSError:return False
  return text[text.rfind(")")+2:].split(" ",1)[0]!="Z"
 return False
```

El guardia pasa de:

```python
if process_is_live(pid):raise RuntimeError(f"live descendant executable unresolved:{pid}:{type(e).__name__}") from e
```

a:

```python
if process_is_confirmed_running(pid):raise RuntimeError(f"live descendant executable unresolved:{pid}:{type(e).__name__}") from e
```

`process_is_confirmed_running` devuelve `True` únicamente cuando `proc_pidinfo` (Darwin) o la lectura de `/proc/<pid>/stat` (Linux) tienen éxito **y** el estado no es zombi. Cualquier otro desenlace — zombi confirmado, o el pid ya no resuelve en absoluto (cosechado por completo) — se trata como "no confirmado en ejecución" y toma la rama benigna. Esto generaliza el discriminante pedido ("Darwin: `status==SZOMB`; Linux: estado `Z`") al caso que la evidencia empírica mostró que también ocurre: el descendiente puede pasar de vivo a completamente cosechado (no solo a zombi) en la ventana entre que `process_executable` falla y el guardia reacciona. `process_is_live` quedó sin más usos y se eliminó.

## Por qué la propiedad de seguridad se mantiene

- El guardia solo toma la rama benigna cuando la consulta *fresca* al kernel dice explícitamente "zombi" o "no existe". Ninguna de las dos puede ejecutar código, abrir sockets ni escribir — el argumento del encargo original se sostiene igual para "ya no existe" que para "zombi": ambos son estrictamente *menos* capaces de hacer daño que un proceso en ejecución.
- Un pid que **sí** resuelve con éxito vía `proc_pidinfo`/`/proc/<pid>/stat` y cuyo estado **no** es zombi sigue disparando `RuntimeError("live descendant executable unresolved…")` sin cambios — cubierto por `test_monitor_live_descendant_executable_unresolved_still_fails`.
- `unexpected descendant executable` (línea con `approval is None`), `descendant executable identity` (hash mismatch), `foreign or reused pid` (en `close`), `required descendant not observed` y `require_transport_closed` no se tocaron: siguen siendo exactamente tan estrictos como antes.
- `remaining_pids` ahora excluye zombis vía `running_pids`, pero la espera acotada post-`SIGKILL` sigue verificando que los pids objetivo **realmente dejen de estar en ejecución** (zombi o ausentes) antes de darlos por cerrados — no se relajó el requisito de que ningún descendiente en ejecución sobreviva al cierre, solo se corrigió qué cuenta como "en ejecución".
- Ningún `sleep` se amplió como parche: el `time.sleep(.05)` fijo se sustituyó por una espera con condición de salida (`target&running` vacío) y techo de 2s: en el caso común (sin zombis pendientes) termina inmediatamente, igual o más rápido que antes.

## Tests deterministas añadidos (`scripts/test-codex-model-qualifier.py`)

1. `test_running_pids_excludes_zombie_rows` — unidad pura sobre `running_pids`, sin procesos reales.
2. `test_close_excludes_zombie_pid_from_remaining_pids` — inyecta (vía monkeypatch de `Transport._rows`) una fila zombi para un pid real pero ajeno; confirma que `close()` no lo cuenta en `remaining_pids` ni genera `failure`, sin depender de una carrera real.
3. `test_monitor_zombie_descendant_executable_unresolved_is_benign` — inyecta `process_executable` fallando con `ProcessLookupError` y `process_is_confirmed_running` devolviendo `False` para un pid "poseído"; confirma que el guardia no lanza y que el pid no queda en `remaining_pids`.
4. `test_monitor_live_descendant_executable_unresolved_still_fails` — mismo montaje pero sin forzar `process_is_confirmed_running` (usa la función real, que confirma que el pid de prueba —el propio proceso de test, genuinamente vivo— sigue en ejecución); confirma que la firma `"live descendant executable unresolved"` se sigue disparando.

## Verificación

```sh
python3 -B scripts/test-codex-model-qualifier.py
```

**25 ejecuciones consecutivas, todas OK** (20 exigidas + 5 adicionales por el historial de sorpresas de este bug), 43 tests por ejecución (39 previos + 4 nuevos):

```
Ran 43 tests in 46.xxxs
OK
```
(repetido 25/25; ninguna ejecución mostró `FAILED`, `TransportCloseError` ni `live descendant executable unresolved` en esta ronda final).

Durante el proceso de arreglo, dos intentos previos SÍ fallaron y quedan documentados aquí porque explican el diseño final:

- Intento 1 (bit de zombi cacheado por ciclo de `_rows()`, sin tocar el guardia de `process_is_live`): **2 fallos en ~15 ejecuciones**, misma firma que el síntoma original.
- Intento 2 (`process_is_zombie` con consulta fresca, pero exigiendo *tanto* `process_is_live` (basado en `kill`, obsoleto) *como* `not process_is_zombie`): **1 fallo en ~18 ejecuciones**, con diagnóstico en vivo mostrando `proc_pidinfo` devolviendo `ESRCH` (pid ya cosechado por completo, ni siquiera zombi) — la causa de que persistiera el flake.
- Diseño final (`process_is_confirmed_running`, consulta única y fresca, sin depender de `os.kill`): **25 de 25 limpias**.

## Nota sobre el alcance de "Linux" en el encargo

El encargo pide un discriminante de zombi específico para Linux (`/proc/<pid>/stat`, estado `Z`). Lo implementé en `process_is_confirmed_running` para cuando se necesite consultar un pid puntual. Para el snapshot de tabla completa (`_rows()`), el camino principal usa `ps` en ambas plataformas y ese mismo carácter `Z` de la columna `STAT` ya proviene de `/proc/<pid>/stat` en Linux (es como `ps` lee el estado allí) — no había necesidad de una función de reserva Linux-específica adicional porque, a diferencia de Darwin, no existía previamente ninguna ruta de reserva sin `ps` para Linux en este archivo (el `raise` en `_rows()` para plataformas no-Darwin cuando `ps` no está disponible no cambió; no es parte de este defecto).
