# W43 — SonarCloud PR #22: los 2 hallazgos de taint que quedan (lectura-modificación-escritura del propio recibo)

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker de scripts. Orquestador: Claude Opus 5. Sin subagentes, sin segundo plano (Bash `timeout` hasta 600000 ms), sin commit, sin Docker, sin red. **Archivos permitidos (y ningún otro):** `scripts/contract-freeze.py`, `scripts/test-contract-freeze.py`, `scripts/measure-m8-performance.py`, `scripts/test-m8-performance-unit.py`.

## Estado

W38 bajó los hallazgos de 22 a 6; W42 los bajó de 6 a 2 y la cobertura está en 92,8 % (OK). El quality gate del PR #22 sigue en ERROR por una sola condición, `new_security_rating` = E, que arrastran **2 `BLOCKER` `pythonsecurity:S2083`**. Todas las demás condiciones están OK y deben seguir OK.

Los dos restantes son **la misma forma**: el script **lee su propio recibo de disco, lo modifica y lo vuelve a escribir**, y el motor de taint trata `read_text()` como fuente no confiable, de modo que el contenido leído llega al `write_text` de destino. La ruta de destino ya es constante en ambos casos — no la toques, no es el problema.

| # | Sink | Fuente | Qué fluye |
| --- | --- | --- | --- |
| 1 | `contract-freeze.py:394` `SCHEMA_DIFF_PATH.write_text(...)` | `391` `existing = json.loads(SCHEMA_DIFF_PATH.read_text())` | El contenido previo del propio `02-schema-diff.json`, que se relee para fusionar la otra clave y se reescribe entero |
| 2 | `measure-m8-performance.py:669` `COMPARE_OUT_PATH.write_text(...)` | `653` `receipts = [json.loads(path.read_text()) ...]` | Los recibos leídos: de ellos salen `receipts[0]["budgets_sha256"]`, `receipts[0]["profile"]` y los `verdicts` calculados sobre su contenido |

## Qué tienes que hacer

Mismo principio que en W42, aplicado ahora al **contenido releído de disco**: un valor solo deja de estar contaminado cuando **se re-deriva de un dominio cerrado**, no cuando se comprueba y se deja pasar.

1. **`contract-freeze.py`** — al fusionar, no reutilices el objeto leído. Reconstruye `existing` entrada por entrada contra el **esquema cerrado que este mismo script escribe** (`base`, `base_commit`, `head_commit`, `tree_dirty`, `added`, `removed`, `changed`, `unchanged`), quedándote solo con claves de `DIFF_OUT_KEYS` y comprobando el tipo de cada campo (listas de cadenas, cadenas, booleanos, y las filas de `changed` con su propia forma). Un archivo con claves o campos que no cumplan el esquema debe **fallar ruidosamente**: es un recibo corrupto, no algo que haya que arrastrar.
2. **`measure-m8-performance.py`** — re-deriva lo que entra en `payload`: `budgets_sha256` contra un patrón `^sha256:[0-9a-f]{64}$` con `match.group(0)`; `profile` por pertenencia a `PROFILE_CHOICES`; y las filas de `verdicts` reconstruidas desde sus campos conocidos con tipos comprobados (`outcome` pertenece a un conjunto cerrado de resultados; las magnitudes son numéricas). Si un recibo no cumple, falla ruidosamente.

En los dos casos el saneamiento es también una mejora real: los scripts pasan a validar sus propios recibos en vez de confiar en que nadie los haya tocado.

## Restricciones inamovibles

- `docs/validation/M8/freeze-0.8.0.json` **no puede cambiar ni un byte**. Verifícalo: copia el manifiesto, `python3 -B scripts/contract-freeze.py generate`, comprueba con `diff` que solo difieren `generated_utc`/`head_commit`/`tree_dirty`, y **restaura la copia**.
- No toques `sonar-project.properties` y **no excluyas ni silencies** ningún hallazgo (nada de `# NOSONAR` para esquivar estos dos): se arregla el código.
- Comportamiento observable idéntico: mismos recibos, mismos mensajes, mismos códigos de salida.

## Verificación obligatoria (ejecútala toda y pega las cifras)

```sh
python3 -B scripts/test-contract-freeze.py
python3 -B scripts/test-m8-performance-unit.py
python3 -B scripts/contract-freeze.py verify --strict
python3 -B scripts/docs-hygiene.py links-check
```

Añade **tests nuevos** para los caminos de rechazo que introduzcas (recibo con clave desconocida, campo de tipo incorrecto): si el fallo ruidoso no está probado, no está hecho.

Informe (mapa hallazgo → cambio → por qué el motor deja de ver el flujo, y las cifras de cada comando) en `docs/validation/M8/delegation/W43-sonar-taint-receipt-readback/report.md` y en tu última respuesta. No commit.
