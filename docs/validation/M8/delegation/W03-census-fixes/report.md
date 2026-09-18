# W03 — informe del worker (Claude Sonnet 5, Medium; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort medium --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md`. Inicio 2026-09-14T14:13:45Z, fin 2026-09-14T14:35:33Z, exit 0, 49 turnos, 1306413 ms, permission_denials: 2 (sin impacto).

---

All edits look correct and scoped. Report follows.

## Informe

**Task:** F1–F4 del censo M8-01 — corregir texto desactualizado (conteos de tools, ADR-README, `--help`, estado del roadmap) sin reescribir hechos históricos.

**Result:** Las cuatro correcciones aplicadas; verificación foreground en verde.

**Files changed:**
- `crates/mcp-server/src/main.rs:59` — añadidas las cinco `rust.analyzer.*` a `Available tools`, ampliado el paréntesis con los requisitos de runtime M6 y del grant `--allow-analyzer-action-write`.
- `docs/tools.md:4-7` — separada la release `0.3.0` (31) del checkout de desarrollo (36, +M6), con enlace a `validation/M6/handoff.md`; `docs/tools.md:138` — «hoy de 31 tools» → «hoy de 36 tools».
- `docs/architecture.md:291-293` — «checkout `0.3.0`, que registra 31 tools» → 36 (31 M1–M5 + 5 M6); `docs/architecture.md:333` — «registra las 31 tools» → 36.
- `docs/adr/README.md` — añadidas viñetas ADR-078, ADR-079, ADR-081, ADR-085 en orden numérico, mismo formato que las vecinas.
- `docs/roadmap/m2-m8.md:12` — «Estado: M2 Done local; M3–M8 Planned/Conditional» → «Estado: M2–M6 Done; M7 Deferred con decisión; M8 en curso». Solo esa línea del encabezado; no toqué la tabla de hitos (ya modificada por trabajo previo no mío en la rama).

**Frases históricas que dejé intactas y por qué:**
- `docs/tools.md:1645` («después de las 31 tools M1–M5») — describe el baseline estructural antes de M6, sigue siendo exacto; la frase siguiente ya dice «pasa a 36».
- `README.md:18-26, 58-63, 568-571` — ya distinguía correctamente release `0.3.0` (31) de checkout de desarrollo (36); no requería cambios.
- `docs/client-configuration.md:243-244` («27 → 31 con las cuatro de M5») — hito histórico M4→M5, no describe el checkout actual.
- `docs/client-configuration.md:316` («tools/list devuelve 31 definiciones») — está en la sección de configuración M5, describe el milestone M5 (27 anteriores + 4 nuevas), no el checkout actual.
- `docs/client-configuration.md:418` («imagen M5 sigue sirviendo las 31 tools anteriores») — describe el comportamiento correcto y vigente de un host con imagen M5 (sin M6), no un conteo total del checkout.

**Salida de los cuatro comandos:**
```
$ cargo fmt --all -- --check
(sin salida — limpio)

$ cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 11m 48s
(sin warnings)

$ cargo test -p rust-engineering-mcp --test cli --locked --offline
running 13 tests
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.12s

$ python3 -B scripts/docs-hygiene.py links-check
links-check: 2543 links resolved; 0 broken in living documents; 5 point at evidence excluded by .gitignore; 459 broken in frozen records
```
`cargo run -q -p rust-engineering-mcp --locked --offline -- --help | grep -c 'rust.analyzer.action.apply'` → `1` ✓

**Risks:** Ninguno detectado; `links-check` reporta 0 rotos en documentos vivos (los 459 "broken in frozen records" son preexistentes en registros congelados fuera de alcance).

**Open issues:** `grep -rn "Available tools" crates scripts` solo encontró `crates/mcp-server/src/main.rs:59` (que edité). No hay ningún test o snapshot en `crates` ni `scripts` que fije ese literal de `--help` con las 31 tools — `crates/mcp-server/tests/cli.rs` solo comprueba substrings estables (`rust.project.open`, etc.), y `crates/mcp-server/tests/protocol.rs` ya listaba las cinco `rust.analyzer.*` mediante `tools/list`, pero ese archivo no está en mi lista de archivos editables, así que no lo toqué; no requería cambio de todos modos. No se hizo commit.
