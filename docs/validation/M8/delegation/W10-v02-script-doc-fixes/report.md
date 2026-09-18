# W10 — informe del worker (Claude Sonnet 5, Medium; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort medium --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md`. Inicio 2026-09-14T17:40:57Z, fin 2026-09-14T17:46:21Z, exit 0, 64 turnos, 322922 ms, permission_denials: 6. Informe escrito por el propio worker:

---


Worker: Claude Sonnet 5. Archivos tocados: `scripts/contract-freeze.py`,
`scripts/test-contract-freeze.py`, `scripts/gate.py` (solo la etapa
`contract-freeze`), `docs/validation/M8/01-census.json`, `CHANGELOG.md`,
`docs/tools.md`, `docs/compatibility.md`. No se regeneró
`freeze-0.8.0.json` ni `02-schema-diff.json`. No commit.

## P2-2 — `contract-freeze.py verify`: reclasificación

`bucket()` ya no decide con la clase *actual*: para diffs de campo usa la
clase **registrada** (`old["stability"]`) en el manifiesto, y se añadió una
categoría nueva `class_changed` que se llena siempre que
`recorded[name]["stability"] != current[name]["stability"]` para un nombre
común. `class_changed` hace fallar `verify` **con o sin `--strict`**. Test
`test_reclassified_tool_fails_verify_even_without_strict`: mueve
`rust.example.stable` a `PREVIEW_NAMES` (equivalente a reclasificar
`rust.check`), cambia su `inputSchema` y confirma `status: failed` sin
`--strict` con una entrada en `class_changed`.

## P2-3 — `gate.py`: etapa `contract-freeze` obligatoria

Se quitó el `if freeze_manifest.exists():`; la etapa corre siempre (diff de
2 líneas, sin tocar `contract-freeze-tests` ni el resto del archivo). Si el
manifiesto falta, `cmd_verify` ahora comprueba `manifest_path.exists()`
antes de leerlo y escribe `verify: manifest not found: <path>` en stderr con
exit 1 (antes lanzaba un traceback de `FileNotFoundError`).
`test_missing_manifest_fails_verify_with_clear_message` cubre el caso.

## P3 — procedencia (`tree_dirty`, `format_version`, `canonical`)

`generate` y `diff` escriben `tree_dirty` (de
`git status --porcelain -- crates/mcp-server/tests/snapshots`) junto a
`head_commit`/`base_commit`. `verify` ahora rechaza (`format_errors`,
siempre fatal) un manifiesto cuyo `format_version` no sea `1` o cuyo
`canonical` no coincida con `CANONICAL_DESCRIPTION`.
`test_format_version_mismatch_fails_verify` y
`test_canonical_mismatch_fails_verify` lo prueban.

## P3 — `git show` en bytes, `--base` validado

`git_bytes()` corre `subprocess.run(["git", *args], capture_output=True)`
sin `text=True`; `load_ref_tools` decodifica UTF-8 explícitamente antes de
`json.loads` y hashea los bytes crudos para `snapshot_sha256`. `resolve_commit`
valida `--base` con `git rev-parse --verify --end-of-options <ref>^{commit}`
(falla con `SystemExit` y mensaje claro si no es un commit-ish válido) antes
de usar el SHA resuelto; `git show`/`git ls-tree` reciben ese SHA con
`--end-of-options` para blindarse contra inyección de opciones.

## P3 — bytes idénticos en `diff`

`tool_entry` ya guardaba `snapshot_sha256`; `cmd_diff` ahora compara también
ese campo y añade `bytes_identical: bool` a cada entrada de `changed` y de
`unchanged` (el formato de `unchanged` pasa de lista de nombres a lista de
objetos `{name, bytes_identical}` — el orquestador regenerará
`02-schema-diff.json` tras el commit).

## P3 — tests añadidos

`description`/`annotations`/`outputSchema` en un `stable`
(`test_stable_description_change_fails_verify`,
`test_stable_annotations_change_fails_verify`,
`test_stable_output_schema_change_fails_verify`); desajuste solo de conteo
(`test_count_only_mismatch_fails_verify`); reclasificación (arriba);
manifiesto ausente (arriba); `diff` contra `HEAD` real
(`DiffTests.test_diff_against_head_reports_the_five_dirty_analyzer_tools`:
0 added/removed, 5 `changed` en `description` — los cinco `rust.analyzer.*`
sin commit — y 31 `unchanged`, cada uno con `bytes_identical`); vector
canónico no-ASCII `{"café": "ñ"}`
(`test_hash_of_a_non_ascii_value_matches_a_fixed_vector`, hash calculado por
`hashlib.sha256` sobre el mismo literal `ensure_ascii=False`).

Total: 20 tests en `test-contract-freeze.py` (antes 9), todos en verde.

## P2-1 — censo: `executes_project_code`

Añadido `executes_project_code` (bool) y `executes_project_code_reason`
(frase por tool) a los 36 elementos de `tools[]`. `true` exactamente para
`rust.check`, `rust.clippy`, `rust.test`, `rust.test.nextest`,
`rust.quality.gate`, `rust.quality.gate.v2`, `rust.coverage`,
`rust.semver.check`, `rust.mutation.test`, `rust.miri`,
`rust.benchmark.run`, `rust.profile.flamegraph`, `rust.binary.bloat`,
`rust.fix.apply` (14); `false` para las 22 restantes, incluidas las cinco
`rust.analyzer.*` (build scripts/proc macros/check-on-save deshabilitados),
`rust.fmt.check`/`rust.fmt.apply` (solo rustfmt) y `rust.benchmark.compare`
(sin proceso). Verificado: `sum(...) == 14`. El diff del JSON solo añade las
dos claves nuevas al final de cada objeto tool (las líneas `}`→`},` que
aparecen como cambiadas son ese efecto colateral de formato, no reordenamiento).

## P2-4 — docs: clase del `contract` y cadena de verificación

`docs/tools.md` §«Clases de estabilidad y documento de contrato»,
`docs/compatibility.md` (párrafo del documento de contrato) y la viñeta del
subcomando en `CHANGELOG.md` declaran ahora que el subcomando `contract` y
su documento (`document_kind: rust_engineering_capabilities`,
`format_version: 1`) son `stable` desde 0.8.0, listan los diez campos del
documento (`document_kind`, `format_version`, `server_version`,
`protocol{primary_version, negotiable_versions, sdk}`,
`tools{name → stability, annotations, input_schema_sha256,
output_schema_sha256, description_sha256, executes_project_code,
requires_runtime}`, `resources[]{uri_template, stability}`, `tool_count`)
con la semántica de `executes_project_code`, y corrigen la cadena de
verificación a sus tres eslabones reales: protocol tests (servidor vivo ↔
snapshots), `tests/cli.rs` (`contract --json` ↔ snapshots) y la etapa
`contract-freeze` (snapshots ↔ manifiesto).

## P3 — redacción CHANGELOG

Ejecuté `python3 -B scripts/contract-freeze.py diff --base v0.3.0 --out
<archivo>` tras los cambios de código: 30 `unchanged`, los 30 con
`bytes_identical: true`; 1 `changed` (`rust.binary.bloat`, esperado,
`bytes_identical: false`); 5 `added` (los `rust.analyzer.*`). La afirmación
«30 contratos `stable` byte-idénticos a `0.3.0`» queda confirmada por bytes
y se mantiene sin cambio de redacción. «Las 31 tools restantes quedan
`stable`» gana la cláusula «condicionadas a superar la matriz de clientes
stock M8-04 antes de RC1 (ADR-086 §1)» en CHANGELOG y compatibility.md. La
viñeta de `--rust-image` ahora dice que el flag existe desde M1 y que lo
nuevo en 0.8.0 es la imagen de runtime M6 que admite (ADR-085).

## Verificación ejecutada (foreground, todo en verde)

- `python3 -B scripts/test-contract-freeze.py` → 20 tests OK.
- `python3 -B scripts/test-gate-reporting.py` → 13 tests OK.
- `python3 -B scripts/contract-freeze.py verify docs/validation/M8/freeze-0.8.0.json`
  → `{"status": "passed", "format_errors": [], "class_changed": [],
  "stable_changed": [], "preview_changed": []}` (el manifiesto actual sigue
  siendo válido bajo las reglas nuevas).
- `python3 -c "...sum(t['executes_project_code'] ...)"` → `14`.
- `python3 -B scripts/docs-hygiene.py links-check` → `0 broken in living
  documents` (459 rotos en frozen records y 5 excluidos por `.gitignore` son
  preexistentes, no tocados por este cambio).

No se hizo commit.
