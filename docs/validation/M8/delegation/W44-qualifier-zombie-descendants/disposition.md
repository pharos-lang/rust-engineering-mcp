# W44 — disposición del orquestador

Invocación: `claude -p --model sonnet --effort high --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md` (CLI 2.1.274). Inicio 2026-09-18T13:25:22Z, fin 14:45:59Z, exit 0, 158 turnos, 4 835 903 ms, modelos `claude-sonnet-5` (+ auxiliar `claude-haiku-4-5`), `permission_denials: 8` (ocho Bash fuera del allowlist —heredocs de diagnóstico, `ps` con `awk`, el bucle de 20 ejecuciones—; el worker rehízo las 25 ejecuciones una a una con el comando admitido).

## El worker corrigió el diagnóstico del orquestador

El encargo afirmaba que bastaba distinguir el estado zombi. **Era correcto en la causa pero incompleto en la ventana**, y el worker lo demostró empíricamente en vez de aceptarlo: dos implementaciones fieles a la letra del encargo —cachear el bit de zombi de una fila de `_rows()`, y exigir `process_is_live and not process_is_zombie`— **seguían fallando** con la misma firma. El diagnóstico en vivo mostró que, para cuando el guardia reacciona, el descendiente con frecuencia ya está **cosechado por completo** (`proc_pidinfo` devuelve ESRCH), no meramente zombi.

Esto se registra porque es la parte valiosa del paquete: el encargo del orquestador habría producido un arreglo que reduce el flake sin eliminarlo, y el worker paró, midió y lo dijo en vez de dar por buena la instrucción.

## Cambio aceptado

- `process_is_live` (basada en `os.kill`) **eliminada**: era la fuente de la señal obsoleta, porque `os.kill(pid,0)` tiene éxito sobre un zombi.
- Nueva `process_is_confirmed_running(pid)`: consulta fresca única (`proc_pidinfo` en Darwin, `/proc/<pid>/stat` en Linux) que devuelve `True` **solo** si el pid resuelve y no está en estado zombi. Zombi o desaparecido ⇒ rama benigna.
- `_rows()` transporta el bit de zombi (columna `stat=` de `ps`, campo `status` en `darwin_process_rows`), y `remaining_pids` lo excluye vía `running_pids()`.
- El `time.sleep(.05)` fijo pasa a espera **acotada y condicionada** (techo 2 s): la corrección ya no depende de una duración.

## La propiedad de seguridad se mantiene

El arnés garantiza que ningún descendiente **en ejecución** queda sin identificar ni sin matar. Un zombi no ejecuta código: no abre sockets, no escribe, no sobrevive a la fase. Excluirlo hace la comprobación más precisa, no más laxa. Verificado por el orquestador sobre el diff: un pid en ejecución con ejecutable no resoluble **sigue** lanzando `live descendant executable unresolved`; intactos `unexpected descendant executable`, `descendant executable identity`, `foreign or reused pid`, `required descendant not observed` y `require_transport_closed`.

Cambio menor de semántica en el recibo, aceptado: `remaining_pids_observed_before_kill` ya no incluye zombis (antes se intentaba `os.kill` sobre ellos, inútilmente). Es más exacto.

## Verificación

| Fuente | Resultado |
| --- | --- |
| Worker | 25 ejecuciones consecutivas OK, 43 tests cada una (39 previas + 4 deterministas nuevas) |
| Orquestador (independiente) | **9 ejecuciones consecutivas OK**, 43 tests cada una. Tasa base previa al arreglo: **5 fallos / 14 ejecuciones** más el fallo dentro del gate (~36 %); a esa tasa, 9 limpias seguidas tienen ~2 % de probabilidad por azar, así que la evidencia distingue el arreglo de la suerte |

Los 4 tests nuevos son deterministas (presentan un pid en estado zombi por inyección, sin depender de ganar una carrera real): un zombi queda excluido de `remaining_pids` y no dispara el guardia; un pid en ejecución con ejecutable no resoluble sigue fallando.

## Clasificación del defecto

**Latente, no regresión de esta rama.** `scripts/codex-model-qualifier.py` y su test eran byte-idénticos a `7c477db`, donde la etapa pasó 39/39 el 2026-09-15. Lo destapó el salto del host a macOS 27.0 (y a Xcode completo), que cambió la temporización de salida y cosecha de procesos.

**La carrera producía falsos fallos, nunca falsos aciertos**: un descendiente real no identificado siempre habría fallado. Por tanto **los recibos de M8-04 anteriores siguen siendo válidos**; lo que estaba roto era la capacidad de volver a pasar, no lo que se pasó.
