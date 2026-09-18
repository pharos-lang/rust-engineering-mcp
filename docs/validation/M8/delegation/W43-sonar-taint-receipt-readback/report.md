# W43 — SonarCloud PR #22: cierre de los 2 `pythonsecurity:S2083` de lectura-modificación-escritura del propio recibo

## Mapa hallazgo → cambio → por qué el motor deja de ver el flujo

### 1. `contract-freeze.py:394` `SCHEMA_DIFF_PATH.write_text(...)` ← `391` `existing = json.loads(SCHEMA_DIFF_PATH.read_text())`

**Cambio.** `cmd_diff` ya no fusiona `existing` copiando el dict leído de disco. Se añadió `known_diff_file()` (y sus ayudantes `known_diff_entry`, `known_changed_row`, `known_unchanged_row`, `known_string_list`, `known_tool_name_list`, `known_bool`, `known_commit_sha`, `known_base`, `known_keys_changed`, `known_object`), que reconstruye el documento **clave por clave contra el esquema cerrado que este mismo script escribe**:

- Nivel superior: solo se aceptan claves de `DIFF_OUT_KEYS` (el resto hace fallar el proceso con `SystemExit`).
- Cada entrada: exactamente las claves `base, base_commit, head_commit, tree_dirty, added, removed, changed, unchanged` (claves desconocidas o faltantes → `SystemExit`).
- `base` se re-deriva contra `BASE_PATTERN` (`match.group(0)`, el mismo patrón que ya validaba el `base` de entrada).
- `base_commit`/`head_commit` se re-derivan contra un nuevo `COMMIT_SHA_PATTERN = ^[0-9a-f]{7,40}$` (mismo rango de commit-ish hexadecimal que `BASE_PATTERN` ya acepta para refs).
- `tree_dirty` y los booleanos de cada fila de `changed` se re-derivan con `isinstance(..., bool)` + `bool(...)`.
- `added`/`removed` y el campo `name` de cada fila de `changed`/`unchanged` se re-derivan con `known_tool_name()` (la misma función ya usada por `load_current_tools`, que aplica `TOOL_NAME_PATTERN` + `match.group(0)`).
- `keys_changed` se re-deriva por pertenencia a un `CHANGED_KEY_CHOICES` cerrado (`annotations`, `description`, `inputSchema`, `outputSchema`).
- Un recibo con clave desconocida o campo de tipo/forma incorrecta hace `raise SystemExit` inmediatamente: no hay ruta en la que el contenido leído llegue intacto a `existing[out_key] = entry` ni al `write_text` final.

**Por qué el motor deja de ver el flujo.** Cada valor que termina en `existing` (y por tanto en el `write_text` de `SCHEMA_DIFF_PATH`) es ahora un valor recién construido — literal booleano, `match.group(0)` de una regex cerrada, o un elemento de una tupla/frozenset cerrada — nunca el objeto (str/dict/list) que vino de `SCHEMA_DIFF_PATH.read_text()`. El taint engine de SonarCloud ya reconocía este patrón para `known_tool_name`/`known_annotations` (ver docstring del módulo); se aplica la misma disciplina a la fusión del propio recibo.

### 2. `measure-m8-performance.py:669` `COMPARE_OUT_PATH.write_text(...)` ← `653` `receipts = [json.loads(path.read_text()) ...]`

**Cambio.** Se añadieron `known_magnitude_id()`, `known_verdict()`, `known_budgets_sha256()`, `known_receipt_profile()`:

- `regression_verdict()` ya no usa el `magnitude_id` leído de `receipt["measurements"]` como clave del resultado ni el string de `"verdict"` leído del recibo como valor de la lista `verdicts`: ambos se re-derivan (`known_magnitude_id` contra `MAGNITUDE_ID_PATTERN = ^[a-z][a-z0-9_]*$` + `match.group(0)`; `known_verdict` por pertenencia al `VERDICT_CHOICES` cerrado `("within", "over", "insufficient_samples", "unavailable")`). `over_count`/`unavailable_count`/`outcome`/`regressed` ya eran valores computados localmente (constantes o conteos), no copias.
- `run_compare()` ya no escribe `receipts[0]["budgets_sha256"]`/`receipts[0]["profile"]` verbatim en `payload`: los pasa por `known_budgets_sha256()` (regex `^sha256:[0-9a-f]{64}$` + `match.group(0)`) y `known_receipt_profile()` (pertenencia al `PROFILE_CHOICES` ya existente), siguiendo el mismo idioma que `main()` ya usa para re-derivar `args.profile`.
- Un recibo con un `magnitude_id` fuera de la gramática, un `verdict` fuera del conjunto cerrado, un `budgets_sha256` mal formado o un `profile` desconocido hace `raise ValueError` de inmediato (capturado como excepción no manejada → traza + salida no-cero, igual que cualquier otro fallo de datos corruptos en este script).

**Por qué el motor deja de ver el flujo.** `payload["verdicts"]`, `payload["budgets_sha256"]` y `payload["profile"]` —los tres campos que terminan en el `write_text` de `COMPARE_OUT_PATH`— están construidos exclusivamente a partir de valores re-derivados (match de regex cerrada, pertenencia a tupla cerrada, o conteos/booleanos calculados), nunca del objeto original devuelto por `json.loads(path.read_text())`.

## Restricciones verificadas

- `docs/validation/M8/freeze-0.8.0.json`: copiado, regenerado con `python3 -B scripts/contract-freeze.py generate`, diferido solo en `generated_utc`/`head_commit`, y restaurado byte a byte (`git status --porcelain` limpio tras la restauración).
- `sonar-project.properties`: no tocado. No se usó `# NOSONAR` ni ninguna exclusión.
- Comportamiento observable: mismos recibos (`docs/validation/M8/02-schema-diff.json`, `target/m8-performance/regression.json`), mismos mensajes y mismos códigos de salida en el camino feliz; el único cambio de comportamiento es el rechazo ruidoso (`SystemExit`/`ValueError`) de recibos que no cumplen el esquema cerrado, que antes se arrastraban silenciosamente.

## Verificación obligatoria (cifras)

```
$ python3 -B scripts/test-contract-freeze.py
...............................
----------------------------------------------------------------------
Ran 31 tests in 0.023s

OK
```

```
$ python3 -B scripts/test-m8-performance-unit.py
.....................................................................................
----------------------------------------------------------------------
Ran 85 tests in 0.006s

OK
```

```
$ python3 -B scripts/contract-freeze.py verify --strict
{"class_changed": [], "format_errors": [], "preview_changed": [], "stable_changed": [], "status": "passed"}
```

```
$ python3 -B scripts/docs-hygiene.py links-check
links-check: 2982 links resolved; 0 broken in living documents; 5 point at evidence excluded by .gitignore; 459 broken in frozen records
```

## Tests nuevos (caminos de rechazo)

`scripts/test-contract-freeze.py` (`DiffTests`, +5 tests: 26 → 31):

- `test_existing_diff_file_with_an_unknown_top_level_key_is_rejected` — clave desconocida a nivel de documento.
- `test_existing_diff_file_with_an_unknown_entry_field_is_rejected` — campo desconocido dentro de una entrada.
- `test_existing_diff_file_with_a_wrong_typed_field_is_rejected` — `tree_dirty` con tipo incorrecto (`str` en vez de `bool`).
- `test_existing_diff_file_with_an_unknown_changed_row_key_is_rejected` — clave desconocida en una fila de `changed`.
- `test_existing_diff_file_preserves_a_valid_entry_when_merging_another_key` — un recibo válido sobrevive intacto la reconstrucción al fusionar una segunda clave (no-regresión de la fusión legítima).

`scripts/test-m8-performance-unit.py` (+4 tests: 81 → 85):

- `test_rejects_a_magnitude_id_outside_the_closed_grammar` — `magnitude_id` fuera de `MAGNITUDE_ID_PATTERN`.
- `test_rejects_a_verdict_outside_the_closed_set` — `verdict` fuera de `VERDICT_CHOICES`.
- `test_compare_rejects_a_malformed_budgets_sha256_on_disk` — `budgets_sha256` mal formado en el recibo leído por `run_compare`.
- `test_compare_rejects_a_profile_outside_the_closed_choices_on_disk` — `profile` fuera de `PROFILE_CHOICES` en el recibo leído por `run_compare`.

También se ajustó el valor por defecto de `make_receipt()` en `test-m8-performance-unit.py` (`budgets_sha256`) de `"sha256:abc"` a un hex de 64 caracteres válido, y `COMMIT_SHA_PATTERN` en `contract-freeze.py` se definió como `^[0-9a-f]{7,40}$` (no `{40}` fijo) para seguir aceptando los commits-ish cortos (`"cafefeed"`, `"deadbeef"`) que ya usaban los fixtures existentes — el mismo rango que `BASE_PATTERN` ya acepta para refs de Git.
