# W10b — test `diff` hermético y nota `--human`

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`). Rol: worker de corrección acotada. Orquestador: Claude Fable 5.1. No puedes lanzar subagentes. **Nunca corras comandos en segundo plano.** No hagas commit. **Archivos permitidos:** `scripts/test-contract-freeze.py`, `scripts/contract-freeze.py` (solo si necesitas un punto de inyección mínimo para los dobles), `docs/tools.md`, `docs/compatibility.md`, `CHANGELOG.md`.

## Contexto

V02b (`docs/validation/M8/delegation/V02b-rereview/disposition.md`, P2-N1) detectó que `DiffTests.test_diff_against_head_reports_the_five_dirty_analyzer_tools` en `scripts/test-contract-freeze.py` (~266-280) depende de que los 5 snapshots `analyzer-*` estén modificados sin commit: tras el commit, `changed == []` y el test falla, y con él la etapa obligatoria `contract-freeze-tests` del gate `core`. Además el CI portable clona sin tags ni historia completa, así que ningún test puede depender de `HEAD`, de `v0.3.0` ni de `git`.

## Tareas

1. Reescribe ese test como **hermético**: construye en `tempfile.TemporaryDirectory()` un conjunto «base» y un conjunto «actual» de snapshots ficticios (p. ej. 3 tools base; el actual añade una, cambia `inputSchema` de otra, elimina una y deja una idéntica) y sustituye con `unittest.mock.patch` las funciones que hablan con git (`git_bytes`, `snapshot_names_at_commit`/`load_ref_tools`, `resolve_commit`, `tree_dirty` o como se llamen en `contract-freeze.py`) para que sirvan esos bytes; afirma `added`/`removed`/`changed` (con `keys_changed`, `input_schema_changed`, `bytes_identical: false`) y `unchanged` (con `bytes_identical: true`). Si `cmd_diff` no admite inyección limpia, añade el mínimo parámetro opcional/refactor en `contract-freeze.py` sin cambiar el comportamiento del CLI. Ningún test del archivo debe invocar `git` real ni depender del estado del árbol: revisa los demás casos y corrígelos si lo hacen.
2. Nota `--human`: en `docs/tools.md` (sección del documento de contrato), `docs/compatibility.md` (línea del documento) y CHANGELOG (viñeta del subcomando) añade que la salida `--human` es informativa y **no** forma parte del contrato `stable` (solo `--json` con `format_version: 1` lo es).

## Verificación (foreground)

`python3 -B scripts/test-contract-freeze.py` verde; `python3 -B scripts/test-gate-reporting.py` verde; `git stash`-free: comprueba la hermeticidad ejecutando los tests con `GIT_DIR=/nonexistent python3 -B scripts/test-contract-freeze.py` (deben seguir verdes); `python3 -B scripts/contract-freeze.py verify docs/validation/M8/freeze-0.8.0.json` passed; `python3 -B scripts/docs-hygiene.py links-check` 0 rotos. Escribe tu informe también en `docs/validation/M8/delegation/W10b-hermetic-diff-test/report.md`. No commit.
