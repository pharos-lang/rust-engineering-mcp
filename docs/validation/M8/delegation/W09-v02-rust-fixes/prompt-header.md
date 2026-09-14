# W09 — correcciones V02 en Rust (`crates/mcp-server` únicamente)

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker de correcciones. Orquestador: Claude Fable 5.1. No puedes lanzar subagentes. **Nunca corras comandos en segundo plano: ejecuta `cargo …` en primer plano y espera** (si necesitas más tiempo, usa `timeout` largo en la propia llamada; jamás `run_in_background`). No hagas commit. No hagas Docker. **Solo `crates/mcp-server/`** (src y tests). No toques snapshots, `scripts/`, `docs/`, `Cargo.*`.

## Contexto

Lee `docs/validation/M8/delegation/V02-review-freeze/disposition.md` (filas P2-1 y P3 de Rust) y `report.md` (P2-1, P3 «literales», «cli.rs annotations», «fallo silencioso»). Código: `crates/mcp-server/src/stdio/capability_document.rs`, `stability.rs`, `src/contract_cli.rs`, `tests/cli.rs`.

## Tareas

1. **P2-1 — semántica y valores de `executes_project_code`** (`capability_document.rs`): documenta en un doc-comment la semántica fijada: «`true` si la tool puede ejecutar build scripts, proc macros, tests o binarios del proyecto dentro del guest». Corrige la tabla: `true` para `rust.check`, `rust.clippy`, `rust.test`, `rust.test.nextest`, `rust.quality.gate`, `rust.quality.gate.v2`, `rust.coverage`, `rust.semver.check`, `rust.mutation.test`, `rust.miri`, `rust.benchmark.run`, `rust.profile.flamegraph`, `rust.binary.bloat`, `rust.fix.apply`; **`false`** para `rust.fmt.check`, `rust.fmt.apply`, `rust.benchmark.compare`, las cinco `rust.analyzer.*` y todas las demás. Añade un test que fije **los valores** (la lista exacta de `true`, 14 tools) y no solo los nombres. Comprueba que `requires_runtime` de `rust.benchmark.compare` sea `none` y de las analyzer `analyzer`.
2. **P3 — literales**: los tests deben comparar las cadenas de `PRIMARY_PROTOCOL_VERSION`/`NEGOTIABLE_PROTOCOL_VERSIONS` contra `SUPPORTED_VERSIONS` (o lo que declare `supported_protocol_versions` en `stdio.rs`), `SDK` contra la versión de `rmcp` en `Cargo.lock` (léela en el test con `include_str!("../../../Cargo.lock")` o equivalente estable) y las plantillas de Resources contra `PREFIX`/`QUALITY_PREFIX` de `resources.rs` — igualdad de strings, no longitudes.
3. **P3 — `tests/cli.rs`**: compara también `annotations` del `contract --json` con las del snapshot para las 36 tools.
4. **P3 — fallo silencioso** (`capability_document.rs` ~506-526 o `contract_cli.rs`): si `document()` falla, escribe un mensaje en stderr antes de salir con código 1 (mismo patrón que otros subcomandos).
5. No cambies descripciones ni annotations de tools (los 36 snapshots deben quedar intactos: `git status --short crates/mcp-server/tests/snapshots` debe mostrar solo los 5 `analyzer-*` ya modificados por W05, sin cambios nuevos).

## Verificación (foreground)

```text
cargo fmt --all -- --check
cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-mcp --locked --offline capability
cargo test -p rust-engineering-mcp --locked --offline --test cli
```
Además `cargo run -q -p rust-engineering-mcp --locked --offline -- contract --json | python3 -c "import json,sys;d=json.load(sys.stdin);print(sorted(n for n,t in d['tools'].items() if t['executes_project_code']))"` → exactamente las 14 tools. **Escribe tu informe final también en `docs/validation/M8/delegation/W09-v02-rust-fixes/report.md`** (Task / Result / Files changed / Tests / Salida de comandos / Risks / Open issues) además de en tu última respuesta. No commit.
