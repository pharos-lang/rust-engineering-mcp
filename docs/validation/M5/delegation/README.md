# Delegaciones externas M5

Registro visible en el repo de las revisiones independientes ejecutadas con CLIs
externas, siguiendo el patrón que M3 dejó en `docs/validation/M3/delegation/`.

Cada directorio conserva el `prompt.md` enviado íntegro, el `stdout.log` y el
`stderr.log` del proceso, y —cuando la revisión termina— el veredicto y su
disposición. **Los intentos fallidos también se conservan**: una delegación
denegada por el sandbox o abortada es información sobre el entorno, no basura.

## Por qué modelos de otra familia

G8 exige revisión independiente, y «independiente» es más fuerte cuando el
revisor no comparte familia de modelo con quien implementó. Las cuatro revisiones
G8 anteriores de M5 —dos iniciales y dos re-revisiones— las hizo Claude Opus 5
sobre trabajo de Claude Opus 5. Eso detecta defectos reales, y de hecho los
detectó, pero comparte sesgos.

| Delegación | CLI | Qué revisa | Por qué a un externo |
| --- | --- | --- | --- |
| `m5-bloat-semantics-codex` | codex 0.153.0 | La implementación de ADR-079 | El defecto se encontró donde corregirlo pone una fila de cliente en verde. Alguien de fuera debe comprobar si ese incentivo contaminó la implementación |
| `m5-statistical-criteria-agy` | agy 1.1.27 | Los criterios de ADR-081 y su corrección | El owner escribió un criterio matemáticamente imposible. Que revise la **corrección** quien no la escribió |

## Restricciones observadas del entorno

Se reutilizan las que este proyecto ya registró en M3 y siguen vigentes con las
mismas versiones (codex 0.153.0, agy 1.1.27):

- codex en `--sandbox read-only` sirve para revisar sin riesgo de escritura.
- agy headless (`--print --sandbox`) deniega automáticamente `read_url` y las
  ejecuciones de comando sin sandbox, así que el encargo le dice explícitamente
  que use solo su herramienta de lectura de archivos y no le pide comandos.
