# W03 — correcciones F1–F4 del censo M8-01 (docs + cadena de `--help`)

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`). Rol: worker de correcciones acotadas. Orquestador: Claude Fable 5.1. No puedes lanzar subagentes. **Nunca corras comandos en segundo plano.** No hagas commit. No hagas Docker.

**Archivos que puedes editar (y ningún otro):** `crates/mcp-server/src/main.rs`, `docs/tools.md`, `README.md`, `docs/architecture.md`, `docs/client-configuration.md`, `docs/adr/README.md`, `docs/roadmap/m2-m8.md` (solo la línea «Estado:» del encabezado).

## Contexto

Rama `ai/m8-stabilization` (M8, estabilización hacia 1.0). El censo M8-01 (`docs/validation/M8/01-census.md` §9) detectó texto desactualizado que contradice el estado real: el checkout anuncia **36 tools** (`tools/list` live) — las 31 de `v0.3.0` más las cinco del analyzer M6 (`rust.analyzer.symbols`, `rust.analyzer.references`, `rust.analyzer.diagnostics`, `rust.analyzer.actions`, `rust.analyzer.action.apply`), integradas en `main` por el PR #20 (`e50c3fe`) y calificadas (`docs/validation/M6/handoff.md`). **No reescribas hechos históricos**: «la release `0.3.0` devuelve 31» sigue siendo cierto y se conserva; solo corrige frases que describen **el checkout actual / de desarrollo**.

## Tareas

1. **F2 — `crates/mcp-server/src/main.rs` línea ~59** (`Available tools: …` del `--help`): la lista enumera 31 y omite las cinco `rust.analyzer.*`. Añádelas al final de la lista en este orden exacto: `rust.analyzer.symbols; rust.analyzer.references; rust.analyzer.diagnostics; rust.analyzer.actions; rust.analyzer.action.apply`, antes del paréntesis final. Amplía el paréntesis para que siga siendo exacto: las tools del analyzer requieren además el runtime M6 (`--rust-image` de la imagen M6) y `rust.analyzer.action.apply` requiere el grant `--allow-analyzer-action-write`. Mantén el estilo del literal. Comprueba con `grep -rn "Available tools" crates scripts` si algún test o snapshot fija ese texto y actualízalo si está entre los archivos permitidos; si no lo está, repórtalo.
2. **F1 — conteos «31» que describen el checkout actual**: `docs/tools.md` líneas ~3-4 («El checkout `0.3.0` devuelve 31: añade cinco tools M2, cuatro M3, cinco M4 y cuatro M5») → 36, añadiendo «y cinco M6 (analyzer)» con enlace a `validation/M6/handoff.md`; `docs/tools.md` ~138 («hoy de 31 tools») y ~1645 («después de las 31 tools») — lee el contexto y corrige solo si describe el inventario actual; `README.md:21` (lista «31 tools (las 18 de M1/M2, …»); `docs/architecture.md:291` y `:333` («registra las 31 tools»); `docs/client-configuration.md:316` («`tools/list` devuelve 31 definiciones») y `:418`. Para cada sitio, lee 10 líneas de contexto antes de tocar; si la frase es histórica (release `0.3.0`, evidencia M3/M4/M5 de su momento), déjala. Usa `grep -n '31' <archivo>` para no dejar ninguna otra mención del checkout con 31.
3. **F3 — `docs/adr/README.md`**: faltan las entradas de ADR-078, ADR-079, ADR-081 y ADR-085. Lee cada ADR (título, Status, decisión en una frase) y añade una viñeta por ADR en orden numérico, con el mismo formato que las entradas vecinas (ADR-077/080/082…).
4. **F4 — `docs/roadmap/m2-m8.md` línea 12**: «Estado: **M2 Done local; M3–M8 Planned/Conditional**.» → «Estado: **M2–M6 Done; M7 Deferred con decisión; M8 en curso**.» Nada más en ese archivo.

## Verificación (foreground, todo verde antes de informar)

```text
cargo fmt --all -- --check
cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-mcp --test cli --locked --offline
python3 -B scripts/docs-hygiene.py links-check
```

Después ejecuta `cargo run -q -p rust-engineering-mcp --locked --offline -- --help | grep -c 'rust.analyzer.action.apply'` y confirma `1`.

Informe final: Task / Result / Files changed (con líneas) / Frases históricas que dejaste intactas y por qué / Salida de los cuatro comandos / Risks / Open issues. No commit.
