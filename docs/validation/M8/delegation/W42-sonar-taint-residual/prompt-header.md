# W42 — SonarCloud PR #22: los 6 hallazgos de taint residuales (rating E) tras W38

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker de scripts. Orquestador: Claude Opus 5. Sin subagentes, sin segundo plano (Bash `timeout` hasta 600000 ms), sin commit, sin Docker, sin red. **Archivos permitidos (y ningún otro):** `scripts/contract-freeze.py`, `scripts/test-contract-freeze.py`, `scripts/measure-m8-performance.py`, `scripts/test-m8-performance-unit.py`, `scripts/soak-m8.py`, `scripts/test-m8-rollback-unit.py`.

## Estado

W38 bajó los hallazgos de taint de 22 a 6 y la cobertura de nuevo código de 60,7 % a 93,5 %. El quality gate del PR #22 sigue en **ERROR** por una sola condición: `new_security_rating` = E (5), que arrastran 2 `BLOCKER` + 4 `MAJOR`. Las demás condiciones están OK y deben seguir OK.

Los 6 restantes **no son el mismo defecto que W38 arregló**. En los seis casos la ruta de destino **ya es una constante de módulo bajo `ROOT`**. Lo que el motor sigue siguiendo es **contenido contaminado** (argv, stdin o bytes leídos de disco) que entra en el diccionario que se serializa y se escribe — más **un único caso de ruta realmente contaminada**. Lee cada flujo antes de tocar nada; no repitas la receta de W38 a ciegas.

| # | Regla | Sink | Fuente | Diagnóstico |
| --- | --- | --- | --- | --- |
| 1 | `S2083` BLOCKER | `contract-freeze.py:349` `out_path.write_text(...)` | stdin (`out` key) vía `346` | **Ruta contaminada de verdad**: `out_path = DIFF_DESTINATIONS[out_key]` — el motor propaga el taint de la clave al valor del subíndice |
| 2 | `S2083` BLOCKER | `contract-freeze.py:157` `FREEZE_MANIFEST_PATH.write_text(...)` | `129` `path.read_bytes()` | Ruta constante; el taint es el **contenido** del snapshot (`spec["name"]`, `spec["annotations"]`) que llega al manifiesto |
| 3 | `S2083` + `S8707` | `measure-m8-performance.py:658` `COMPARE_OUT_PATH.write_text(...)` | `630` `parse_args()` / `643` `path.read_text()` | Ruta constante; `payload["receipts"] = receipt_keys` guarda el argv **crudo**, no el validado |
| 4 | `S8707` MAJOR | `measure-m8-performance.py:752` `OUT_PATH.write_text(...)` | `630` `parse_args()` | Ruta constante; valores de argv llegan al recibo |
| 5 | `S8707` MAJOR | `soak-m8.py:647` `OUT_PATH.write_text(...)` | `600` `parse_args()` | Ruta constante; valores de argv (p. ej. `args.profile`) llegan al recibo |

## Qué tienes que hacer

**Objetivo medible: 0 vulnerabilidades de `pythonsecurity:*` en el PR, `new_security_rating` A.** El motor de taint de SonarCloud no entiende «lo validé más arriba y si no lancé una excepción»: solo corta el flujo cuando **el valor que se usa se re-deriva de un dominio cerrado**. Aplica ese principio, que es la extensión natural de la regla de la casa:

1. **`contract-freeze.py:349`** — `DIFF_DESTINATIONS` es degenerado: sus dos claves apuntan al **mismo** `SCHEMA_DIFF_PATH`. Escribe en la constante `SCHEMA_DIFF_PATH` directamente y deja `out_key` como lo que realmente es: una **etiqueta** validada contra un conjunto cerrado que solo selecciona la clave dentro del JSON. Ninguna ruta vuelve a salir de un subíndice con clave contaminada.
2. **Los cinco casos de contenido** — todo valor que aterrice en un recibo debe **re-derivarse de un dominio cerrado**, no copiarse del input:
   - Valor que viene de `choices=[...]` de argparse → guarda el literal de la tupla constante correspondiente (selección por pertenencia), no `args.<x>`.
   - Valor validado por regex → guarda `match.group(0)` del `re.match` que ya haces, no la cadena original. En `receipt_path_for_key` ya existe la validación: haz que devuelva también la clave re-derivada y **guarda esa** en `payload["receipts"]`.
   - Parámetros numéricos → guarda el resultado de `int()`/`float()` (ya sanea), nunca la cadena cruda.
   - `contract-freeze.py` `tool_entry`/`load_current_tools`: `name` debe validarse contra el **conjunto cerrado de nombres conocidos** (el manifiesto es el oráculo: solo puede contener tools conocidas) y usarse el nombre re-derivado de ese conjunto; `annotations` debe reconstruirse a partir de sus claves conocidas con tipos comprobados en vez de copiarse tal cual. Un snapshot con un nombre desconocido debe **fallar ruidosamente**, no colarse.

## Restricción inamovible

`docs/validation/M8/freeze-0.8.0.json` es el oráculo de contratos de M8 y **no puede cambiar ni un byte**. Todo el saneamiento debe preservar los valores exactos. Compruébalo así: guarda una copia del manifiesto, ejecuta `python3 -B scripts/contract-freeze.py generate`, y verifica con `diff` que el archivo resultante es **idéntico**; restaura la copia si algo lo movió. Igual para el comportamiento observable de los tres scripts: mismos recibos, mismos mensajes, mismos códigos de salida.

**No toques `sonar-project.properties`** (la cobertura ya está en 93,5 %) y **no excluyas** ningún archivo para esquivar un hallazgo: se arregla el código, no se silencia el análisis.

## Verificación obligatoria (ejecútala toda y pega las cifras)

```sh
python3 -B scripts/test-contract-freeze.py
python3 -B scripts/test-m8-performance-unit.py
python3 -B scripts/test-m8-rollback-unit.py
python3 -B scripts/contract-freeze.py verify --strict
python3 -B scripts/docs-hygiene.py links-check
```

Más la comprobación de idempotencia del manifiesto descrita arriba. Si un test ya no refleja el contrato, **adáptalo con criterio y dilo**; no lo borres para que pase.

Informe (mapa hallazgo → cambio → por qué el motor deja de ver el flujo, y las cifras de cada comando) en `docs/validation/M8/delegation/W42-sonar-taint-residual/report.md` y en tu última respuesta. No commit.
