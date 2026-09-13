# W12 — admitir la sonda `rustc --print` por lotes de rust-analyzer en el allowlist de guest (ADR-084 §7)

Modelo solicitado: Claude Opus 5 (`claude -p --model opus --effort high`). Rol: worker de implementación sobre una **frontera de seguridad** (el allowlist de argv admitidos dentro del guest, ADR-084 §7, aserción P1). Orquestador: Claude Fable 5.1 (como Opus 4.8). No puedes lanzar subagentes. **Nunca corras un comando en segundo plano. No corras Docker (el orquestador re-corre `m6-runtime`). No hagas commit.**

## Diagnóstico (vinculante — ya establecido por el orquestador)

El corte nativo `m6-03-guest-programs` (`m6_initialize_spawns_only_the_expected_guest_programs`, `crates/execution-adapter/src/analyzer_native.rs:1379`) falló en el gate `full` porque `admitted_argv` NO admite una sonda legítima de rust-analyzer observada en la tabla de procesos del guest:

```
/opt/rust/bin/rustc - --crate-name ___ --print=file-names --target aarch64-unknown-linux-gnu --crate-type bin --crate-type rlib --crate-type dylib --crate-type cdylib --crate-type staticlib --crate-type proc-macro --print=sysroot --print=split-debuginfo --print=crate-name --print=cfg -Wwarnings
```

Es una **consulta de solo lectura** del toolchain que RA ejecuta en `initialize`: lee fuente sintética por stdin (`-`, `--crate-name ___`), no compila nada, no escribe. El recibo de calibración `7f935ac1` pasó por **azar de muestreo**: `docker container top` muestrea instantes cada 25 ms y esta sonda vive brevemente, así que el recibo observó 2 argv y esta corrida observó 4. El corte es **flaky** hasta que el allowlist cubra esta sonda de forma determinista. NO es una regresión ni una brecha: `programs` = `{cargo, rust-analyzer, rustc}`, todos binarios legítimos del toolchain del guest.

## Cambio (una regla de seguridad, precisa y auditable)

Generaliza SOLO el brazo `"/opt/rust/bin/rustc"` de `admitted_argv` (`analyzer_native.rs:61`). Admite un argv de `rustc` si y solo si cumple **una** de estas dos formas:

1. Exactamente `["-vV"]` (la sonda de versión; ya admitida).
2. Es una **consulta `--print` de solo lectura**: contiene ≥1 token `--print <x>` o `--print=<x>`, Y **cada** token del argv pertenece al vocabulario cerrado de consulta:
   - `-` (fuente por stdin), `-vV`, `-O`, `-Wwarnings`, `-Z`, `unstable-options`,
   - `--crate-name` + su valor, `--crate-type` + su valor, `--target` + su valor,
   - `--print` + su valor, y `--print=<x>` (forma con `=`).
   Ningún token puede ser una ruta (empieza con `/` o termina en `.rs`) ni un flag de salida/codegen (`-o`, `--out-dir`, `--emit`, `-L`, `--extern`, cualquier `-C…`). El vocabulario positivo estricto ya excluye todo eso: si aparece cualquier token fuera de la lista, **rechaza**.

La forma con `=` (`--print=sysroot`) y la forma con espacio (`--print sysroot`) deben admitirse ambas. Las cuatro formas de `rustc` ya admitidas (`-vV`, `--print sysroot`, `--print cfg -O`, `-Z unstable-options --print target-spec-json`) DEBEN seguir admitidas bajo la nueva regla. No toques los brazos `rust-analyzer` ni `cargo`.

Implementa la regla de `rustc` en un helper legible (p.ej. `rustc_is_readonly_probe(args: &[&str]) -> bool`) con un parse token a token; documenta en un comentario que solo admite consultas que no compilan ni escriben.

## Tests (extiende el unit test existente, sin Docker)

En `the_guest_argv_allowlist_admits_the_expected_forms_and_nothing_else` (~línea 2208):
- **admitted**: añade la sonda por lotes exacta de arriba, y una variante con `=`/espacio mezclados; conserva las cuatro formas previas.
- **refused**: añade casos que prueban que la generalización NO abre la puerta: la misma sonda pero con `-o /tmp/x`, con `--emit=obj`, con `--out-dir /tmp`, con `-L /source`, con `--extern foo=/x`, con una ruta real `/source/src/lib.rs` en vez de `-`, y `rustc --print` sin más (sin `-` ni vocabulario extraño está bien admitir `--print sysroot`; pero `rustc --print cfg /source/src/lib.rs` debe rechazarse por la ruta). Conserva TODOS los refused actuales (incluido `rustc --crate-name fixture /source/src/lib.rs`).

## Doc (mismo cambio)

Añade una nota fechada breve en el comentario del corte `m6-03` (o donde viva la referencia a ADR-084 §7 en ese archivo) explicando que la sonda `rustc --print` por lotes de RA se admite como consulta de solo lectura. Si `docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md` §7 enumera el allowlist, añade ahí la misma nota fechada 2026-09-12.

## Verificación (foreground, sin Docker)

```text
cargo fmt --all -- --check
cargo check -p rust-engineering-execution --all-targets --locked --offline
cargo clippy -p rust-engineering-execution --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-execution --lib --locked --offline analyzer_native   # los unit tests NO-ignored (allowlist); los #[ignore] no corren
python3 -B scripts/check-architecture.py
```

Reporta: Task / Result / Files changed / La regla exacta implementada (cita el helper) / Tests (admitted+refused añadidos, salida de cargo test/clippy) / Cómo la regla mantiene refused todo lo peligroso (argumento de seguridad) / Risks / Open issues. No commit.
