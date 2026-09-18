# W10b — informe del worker (Claude Sonnet 5, Medium; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort medium … < prompt-header.md`. Inicio 2026-09-14T18:17:59Z, fin 2026-09-14T18:20:47Z, exit 0, 30 turnos, 167381 ms, permission_denials: 0.

---


Worker: Claude Sonnet 5. Origen: V02b (`docs/validation/M8/delegation/V02b-rereview/disposition.md`, P2-N1).

## Problema

`DiffTests.test_diff_against_head_reports_the_five_dirty_analyzer_tools` en
`scripts/test-contract-freeze.py` dependía de que los cinco snapshots
`analyzer-*` estuvieran modificados sin commit en el árbol de trabajo real
(`cmd_diff("HEAD", ...)` invocando `git` de verdad). Tras el commit de esos
snapshots, `changed == []` y el test — y con él la etapa obligatoria
`contract-freeze-tests` del gate `core` — fallaría. El CI portable clona sin
tags ni historia completa, así que ningún test puede depender de `HEAD`, de
`v0.3.0` ni de invocar `git` real.

## Cambios

### `scripts/test-contract-freeze.py`

- `DiffTests` reescrito como hermético. `setUp` construye un conjunto base
  (`keep`, `change`, `remove`) y un conjunto actual (`keep` idéntico,
  `change` con `inputSchema` distinto, `add` nuevo; `remove` ausente) usando
  `tempfile`/`write_tool`, y usa `unittest.mock.patch.object` para sustituir:
  - `CF.SNAPSHOTS_DIR` → directorio temporal con el set "actual"
    (consumido por `load_current_tools`, que solo lee disco).
  - `CF.resolve_commit` → fijo (`"deadbeef"`), sin `git rev-parse`.
  - `CF.snapshot_names_at_commit` → lista fija de rutas del set "base".
  - `CF.git_bytes` → devuelve los bytes fijos del set "base" según la ruta
    pedida en `git show <commit>:<path>`, sin invocar `git` real.
  - `CF.head_commit` → fijo (`"cafefeed"`), sin `git rev-parse HEAD`.
  - `CF.tree_is_dirty` → fijo (`False`), sin `git status`.
  - No fue necesario tocar `contract-freeze.py`: todas las funciones que
    hablan con git ya eran objetos de módulo sustituibles vía
    `mock.patch.object`, así que no hizo falta ningún parámetro de
    inyección nuevo.
  - El único test (`test_diff_reports_added_removed_changed_and_unchanged_tools`)
    afirma `added == ["rust.example.add"]`, `removed == ["rust.example.remove"]`,
    un `changed` con `keys_changed == ["inputSchema"]`,
    `input_schema_changed: true`, `bytes_identical: false`, y un `unchanged`
    con `bytes_identical: true`.
- `GenerateVerifyTests.setUp` también invocaba `cmd_generate`, que llama a
  `head_commit()`/`tree_is_dirty()` — ambas hacían `git` real y fallaban bajo
  `GIT_DIR=/nonexistent`. Se añadieron los mismos dos parches
  (`head_commit`/`tree_is_dirty` fijos) a ese `setUp`.
- `RealRepositoryClassificationTests` no se tocó: llama a
  `CF.load_current_tools()` directamente sobre los snapshots reales del
  repo, sin usar `git` ni depender del estado del árbol (dirty/clean); no
  entra en el alcance de la hermeticidad frente a `git`.
- Se añadió el import `shutil` (limpieza del directorio temporal del
  fixture de `DiffTests`).

### Documentación de `--human`

- `docs/tools.md` (sección "Clases de estabilidad y documento de
  contrato"): la firma del subcomando pasa a
  `contract [--json | --human]` y se añade que solo `--json` (con
  `format_version: 1`) es el contrato `stable`; `--human` es informativo.
- `docs/compatibility.md` (línea del documento de contrato): misma nota
  añadida junto a la descripción de `contract --json`.
- `CHANGELOG.md` (viñeta del subcomando `contract`): firma actualizada a
  `[--json | --human]` y misma nota añadida.

## Verificación (foreground)

- `python3 -B scripts/test-contract-freeze.py` → `OK` (20 tests).
- `python3 -B scripts/test-gate-reporting.py` → `OK` (13 tests).
- `GIT_DIR=/nonexistent python3 -B scripts/test-contract-freeze.py` → `OK`
  (20 tests; confirma hermeticidad frente a git).
- `python3 -B scripts/contract-freeze.py verify docs/validation/M8/freeze-0.8.0.json`
  → `{"status": "passed", ...}`.
- `python3 -B scripts/docs-hygiene.py links-check` → `0 broken in living
  documents` (459 rotos en frozen records y 5 excluidos por `.gitignore` son
  preexistentes, fuera de alcance de este cambio).

## No commit

Sin commit, según instrucción del encargo.
