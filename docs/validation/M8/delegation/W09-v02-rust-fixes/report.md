# W09 — informe del worker (Claude Sonnet 5, High; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort high --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md`. Inicio 2026-09-14T17:40:57Z, fin 2026-09-14T18:13:51Z, exit 0, 85 turnos, 1973539 ms, permission_denials: 13.

---


Worker: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Orquestador: Claude Fable 5.1.

## Task

Cerrar, en `crates/mcp-server` exclusivamente, los hallazgos P2-1 y P3 de Rust registrados en
`docs/validation/M8/delegation/V02-review-freeze/disposition.md` y `report.md`:

1. P2-1 — fijar la semántica y los 8 valores erróneos de `executes_project_code`, con un test que
   fije la lista exacta de las 14 tools en `true`.
2. P3 — comparar los literales de protocolo/SDK/plantillas de Resources contra su fuente de verdad
   por igualdad de strings, no por longitud.
3. P3 — `tests/cli.rs` debe comparar también `annotations` del `contract --json` contra el snapshot.
4. P3 — que `document()`/`contract` escriban un mensaje en stderr antes de salir con código 1 si
   fallan.
5. No tocar snapshots, `scripts/`, `docs/` (salvo este informe) ni `Cargo.*`.

## Result

Las cinco tareas quedaron cerradas. `git status --short crates/mcp-server/tests/snapshots` muestra
únicamente los 5 snapshots `analyzer-*` ya modificados por W05 (ninguno nuevo). No se tocó
`scripts/`, `docs/` (salvo este informe) ni `Cargo.*`. No se hizo commit.

### P2-1 — `executes_project_code`

`capability_document.rs`: la tabla `TOOL_CLASSES` tenía 8 valores `true` incorrectos (confirmando
el conteo exacto que ya daba el revisor V02). Se corrigieron a `false`:

- `rust.fmt.check` (`format::NAME`)
- `rust.fmt.apply` (`mutation::FORMAT_NAME`)
- `rust.benchmark.compare` (`benchmark_compare::NAME`; ya tenía `requires_runtime: None`, correcto)
- `rust.analyzer.symbols`, `rust.analyzer.references`, `rust.analyzer.diagnostics`,
  `rust.analyzer.actions` (las cuatro tools de `analyzer.rs`)
- `rust.analyzer.action.apply` (`mutation::ANALYZER_ACTION_APPLY_NAME`, la quinta tool `preview`,
  definida en `mutation/analyzer_action.rs` pero con nombre `rust.analyzer.*`)

El resultado son exactamente los 14 `true` pedidos: `rust.check`, `rust.clippy`, `rust.test`,
`rust.test.nextest`, `rust.quality.gate`, `rust.quality.gate.v2`, `rust.coverage`,
`rust.semver.check`, `rust.mutation.test`, `rust.miri`, `rust.benchmark.run`,
`rust.profile.flamegraph`, `rust.binary.bloat`, `rust.fix.apply`.

Se reescribió el doc-comment sobre `TOOL_CLASSES` fijando la semántica: "`true` si la tool puede
ejecutar build scripts, proc macros, tests o binarios del proyecto dentro del guest", con la lista
cerrada de 14 nombres y la justificación de los tres casos límite (`fmt.*` solo corre rustfmt,
`benchmark.compare` no lanza proceso, los `analyzer.*` dejan build scripts/proc macros/
check-on-save deshabilitados).

Nuevo test `executes_project_code_matches_the_fixed_fourteen_tool_set` en
`capability_document.rs`: fija la lista ordenada de los 14 nombres en `true` (no solo el conteo) y
comprueba `requires_runtime` de `rust.benchmark.compare` == `None` y de las cinco `analyzer.*` ==
`Analyzer`.

### P3 — literales de protocolo/SDK/plantillas

- `negotiable_versions_match_supported_versions_strings` (reemplaza al test que solo comparaba
  longitudes): serializa cada `ProtocolVersion` de `stdio::SUPPORTED_VERSIONS` con `serde_json` y
  compara la lista completa de strings contra `NEGOTIABLE_PROTOCOL_VERSIONS`; también comprueba que
  `PRIMARY_PROTOCOL_VERSION` es el último elemento.
- `sdk_literal_matches_the_rmcp_version_in_cargo_lock` (nuevo): lee
  `include_str!("../../../../Cargo.lock")`, localiza el bloque `name = "rmcp"` / `version = "..."` y
  compara `SDK` (`"rmcp 3.2.0"`) contra `"rmcp {version}"`.
- `resource_templates_match_the_resources_module_prefixes` (nuevo): compara que las dos entradas de
  `RESOURCE_TEMPLATES` empiecen por `resources::PREFIX`/`resources::QUALITY_PREFIX`. Para que el
  test pudiera nombrar esas constantes se les subió la visibilidad de privada a `pub(super)` en
  `resources.rs` (mismo patrón ya usado por `hex`), sin cambiar su valor ni ningún comportamiento en
  tiempo de ejecución.

### P3 — `tests/cli.rs`: comparar `annotations`

En `contract_json_describes_exactly_the_36_snapshot_tools` se añadió
`assert_eq!(tool["annotations"], snapshot["annotations"], "{name} annotations")` junto a las
comparaciones ya existentes de los tres hashes canónicos, para las 36 tools.

### P3 — fallo silencioso

`capability_document::run`: si `document()` devuelve `Err`, ahora escribe en stderr
`"rust-engineering-mcp contract: failed to build the capabilities document: {mensaje}"` (usa
`error.message` de `ErrorData`, mismo campo que ya se usa en otros tests del crate) antes de
devolver `ExitCode::FAILURE`. Se añadió el mismo tratamiento al fallo (hoy inalcanzable en la
práctica, pero simétrico) de `serde_json::to_vec` al serializar el documento en modo `--json`.

## Files changed

- `crates/mcp-server/src/stdio/capability_document.rs` — doc-comment de semántica, 8 valores de la
  tabla, 3 tests nuevos/reescritos, mensajes de stderr en `run()`.
- `crates/mcp-server/src/stdio/resources.rs` — `PREFIX`/`QUALITY_PREFIX` pasan de privadas a
  `pub(super)` (sin cambio de valor).
- `crates/mcp-server/tests/cli.rs` — una aserción nueva sobre `annotations`.
- `docs/validation/M8/delegation/W09-v02-rust-fixes/report.md` — este informe (única escritura
  fuera de `crates/mcp-server`).

## Tests

```
cargo fmt --all -- --check                                                             # limpio
cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings   # limpio
cargo test -p rust-engineering-mcp --locked --offline capability                       # 9 passed
cargo test -p rust-engineering-mcp --locked --offline --test cli                       # 15 passed
```

## Salida de comandos

```
$ cargo fmt --all -- --check
(sin salida)

$ cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings
    Checking rust-engineering-mcp v0.8.0 (/Users/cburgosro/Projects/rust-mcp/crates/mcp-server)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 23.15s

$ cargo test -p rust-engineering-mcp --locked --offline capability
running 9 tests
test stdio::capability_document::tests::resource_templates_match_the_resources_module_prefixes ... ok
test stdio::capability_document::tests::negotiable_versions_match_supported_versions_strings ... ok
test stdio::capability_document::tests::sdk_literal_matches_the_rmcp_version_in_cargo_lock ... ok
test stdio::capability_document::tests::canonicalize_sorts_nested_keys_regardless_of_insertion_order ... ok
test stdio::capability_document::tests::executes_project_code_matches_the_fixed_fourteen_tool_set ... ok
test stdio::capability_document::tests::canonical_hash_matches_the_python_reference_vector ... ok
test stdio::capability_document::tests::table_matches_the_full_tool_set ... ok
test stdio::capability_document::tests::document_shape_is_stable ... ok
test stdio::capability_document::tests::human_report_lists_every_tool_once ... ok
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 451 filtered out

$ cargo test -p rust-engineering-mcp --locked --offline --test cli
running 15 tests
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo run -q -p rust-engineering-mcp --locked --offline -- contract --json \
    | python3 -c "import json,sys;d=json.load(sys.stdin);print(sorted(n for n,t in d['tools'].items() if t['executes_project_code']))"
['rust.benchmark.run', 'rust.binary.bloat', 'rust.check', 'rust.clippy', 'rust.coverage',
 'rust.fix.apply', 'rust.miri', 'rust.mutation.test', 'rust.profile.flamegraph',
 'rust.quality.gate', 'rust.quality.gate.v2', 'rust.semver.check', 'rust.test', 'rust.test.nextest']

$ git status --short crates/mcp-server/tests/snapshots
 M crates/mcp-server/tests/snapshots/analyzer-action-apply-tool.json
 M crates/mcp-server/tests/snapshots/analyzer-actions-tool.json
 M crates/mcp-server/tests/snapshots/analyzer-diagnostics-tool.json
 M crates/mcp-server/tests/snapshots/analyzer-references-tool.json
 M crates/mcp-server/tests/snapshots/analyzer-symbols-tool.json
```

## Risks

- El `include_str!("../../../../Cargo.lock")` acopla el test al layout físico del workspace
  (4 niveles desde `crates/mcp-server/src/stdio/`) y al formato de `Cargo.lock` v4 (bloques
  `name = "..."` seguidos de `version = "..."`). Si `cargo` cambia ese formato de serialización en
  una versión futura, el test fallará con un mensaje claro (`unexpected Cargo.lock format for rmcp
  version`), no en silencio.
- Subir `PREFIX`/`QUALITY_PREFIX` a `pub(super)` amplía ligeramente su superficie de visibilidad
  (a todo `stdio` en vez de solo `resources`), pero no cambia comportamiento; es el mismo patrón ya
  usado por `hex` en el mismo archivo.
- El mensaje de stderr para el fallo de `document()` es hoy inalcanzable en la práctica: los únicos
  errores posibles (`Unclassified tool`, `tool definition missing name`) están cubiertos por
  `table_matches_the_full_tool_set`, que falla en tiempo de test si alguien añade una tool sin
  clasificar. El cambio deja el subcomando simétrico con el resto del CLI en vez de morir en
  silencio si esa invariante se rompiera igualmente en producción.

## Open issues

Ninguno de los P2/P3 de Rust asignados a W09 queda pendiente. Fuera de alcance de este worker
(asignados a W10 por la disposición): P2-2 (reclasificación en `contract-freeze.py`), P2-3 (etapa
`contract-freeze` omitible en `gate.py`), P2-4 (clase de estabilidad del subcomando `contract` y su
formato en CHANGELOG/docs) y el resto de P3 de Python/documentación (procedencia de
`head_commit`, `git show` con `text=True`, `--base` sin `--end-of-options`, huecos de
`test-contract-freeze.py`, listas duplicadas, promesa de "31 stable" sin condición).
