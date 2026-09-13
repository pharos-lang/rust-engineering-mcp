# W12b — cerrar los dos P3 de V12 sobre el allowlist de argv del guest

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker de implementación sobre la misma frontera de seguridad que W12 (working tree con W12 sin commitear). Orquestador: Claude Fable 5.1 (como Opus 4.8). No puedes lanzar subagentes. **Nunca corras un comando en segundo plano. No corras Docker. No hagas commit.** Lee primero `docs/validation/M6/delegation/V12-review-guest-argv/disposition.md`.

## Cambios (los dos P3 de V12, ambos en `crates/execution-adapter/src/analyzer_native.rs`)

### P3-1 — cerrar `--target` al único triple del guest (corrección de código)

En `rustc_is_readonly_probe`, el brazo `--target` hoy acepta cualquier `plain_word` como valor. Un valor como `hostile` (palabra simple válida) hace que rustc busque `hostile.json` en el disco (`RUST_TARGET_PATH`, el CWD `/source`, o el sysroot) — una **lectura** de una posible ruta del proyecto, aunque no compile ni escriba. Esto contradice la promesa "la única entrada admitida es `-`" del comentario del helper y de la enmienda ADR-084 §7.

Cambia el brazo `--target` para que admita **solo** el triple exacto del guest M6, `aarch64-unknown-linux-gnu` (el único que rust-analyzer usa en esa imagen; las otras sondas ya lo hardcodean). Es decir: `--target` consume su valor y lo admite solo si es igual a esa constante. Define la constante una vez (p.ej. `const GUEST_TARGET_TRIPLE: &str = "aarch64-unknown-linux-gnu";`) y reúsala si ya existe una equivalente en el archivo. Mantén `--crate-name`/`--crate-type` con `plain_word` (esos valores no provocan lecturas de disco).

### P3-2 — documentar el límite del oráculo (nota, no código de control)

El observador (`ProgramObserver`) recibe el argv unido por espacios desde `docker container top`/`ps`, y `admitted_argv` lo re-parte con `split_whitespace`, así que no ve los límites reales de cada argumento (un nombre de archivo con un espacio se re-partiría). No es corregible barato desde fuera del guest (`docker top` es el mecanismo). Añade una nota breve en el comentario del corte `m6_initialize_spawns_only_the_expected_guest_programs` (junto a la nota de muestreo por instantes ya existente) explicando esta limitación de límites-de-argumento del oráculo, en la misma línea que la nota de que la ausencia de procesos es evidencia sobre instantes muestreados. Refleja la misma limitación en una frase de la enmienda 2026-09-12 de ADR-084 §7.

## Tests

En el unit test `the_guest_argv_allowlist_admits_the_expected_forms_and_nothing_else`: añade a **refused** `"/opt/rust/bin/rustc --target hostile --print cfg"` y `"/opt/rust/bin/rustc - --crate-name ___ --target evil --print=cfg"` (target no-guest → rechazado). Conserva en **admitted** la sonda por lotes real (que usa `--target aarch64-unknown-linux-gnu`) y todas las formas previas. No borres ningún caso existente.

## Verificación (foreground, sin Docker)

```text
cargo fmt --all -- --check
cargo check -p rust-engineering-execution --all-targets --locked --offline
cargo clippy -p rust-engineering-execution --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-execution --lib --locked --offline analyzer_native
python3 -B scripts/check-architecture.py
```

Reporta: Task / Result / Files changed / El brazo `--target` exacto después / Tests (refused añadidos, salida de cargo test/clippy) / Risks / Open issues. No commit.
