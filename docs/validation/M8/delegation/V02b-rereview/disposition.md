# V02b — disposición del orquestador (2026-09-14)

Veredicto del revisor: **Block** por una regresión nueva; los cuatro P2 de V02
confirmados como cerrados.

| ID | Sev | Disposición | Dónde se cierra |
| --- | --- | --- | --- |
| P2-N1 `DiffTests` depende del árbol sin commit (`cmd_diff("HEAD")` espera 5 `changed`) | P2 gate | **Aceptado** — causa: instrucción del propio encargo W10 (error del orquestador). El gate `core` en curso se abortó antes de tocar `scripts/` (guard `source_inputs_unchanged`). Arreglo: test hermético con dobles de `git_bytes`/`snapshot_names_at_commit` sobre fixtures en directorio temporal, sin depender de HEAD ni de tags (el CI portable clona sin tags) | W10b |
| P3 `--human` fuera del contrato `stable` | P3 | Aceptado: una frase en tools.md/compatibility/CHANGELOG | W10b |
| P3 JSON de evidencia generados con el script anterior (`head_commit dbc17f5`, sin `tree_dirty`/`bytes_identical`) | P3 | Aceptado: el orquestador regenera `freeze-0.8.0.json` y `02-schema-diff.json` **tras el commit del código** y los commitea aparte, con `tree_dirty: false` | orquestador |
| P3 fallo de escritura de stdout sin stderr; recuentos del manifiesto no verificados; renombrado de preview con mismo conteo | P3 | Anotados como deuda menor (M8-09 usa `--strict`; `class_changed` cubre el cambio de clase) | matriz §Deuda |
