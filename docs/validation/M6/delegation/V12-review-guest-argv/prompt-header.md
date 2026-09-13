# V12 — revisión independiente read-only del allowlist de argv del guest (W12, ADR-084 §7)

Modelo solicitado: Claude Opus 5 (`claude -p --model opus --effort high`). Rol: **revisor de seguridad read-only**. NO edites, NO hagas commit, NO corras Docker ni gates. Orquestador: Claude Fable 5.1 (como Opus 4.8). No puedes lanzar subagentes.

## Contexto

El corte de calibración M6 `m6-03-guest-programs` afirma (ADR-084 §7, hallazgo **P1**) que dentro del guest de rust-analyzer solo corren programas de una lista cerrada de argv (`admitted_argv` en `crates/execution-adapter/src/analyzer_native.rs`). W12 generalizó el brazo `/opt/rust/bin/rustc` para admitir la sonda `--print` por lotes que rust-analyzer 1.98.1 ejecuta en `initialize` (lee fuente sintética por stdin `-`, imprime y sale; no compila ni escribe). Antes solo se admitían cuatro formas fijas de `rustc`, y el corte era flaky porque `docker container top` muestrea instantes y la sonda vive brevemente.

## La ÚNICA pregunta de seguridad

¿Puede `rustc_is_readonly_probe` (el helper nuevo) admitir algún argv de `rustc` que **compile**, **escriba un archivo**, **lea una ruta del proyecto**, o **cuele un valor peligroso como flag**? Analiza el parse token a token, `plain_word`, `STOPPING_PRINT_KINDS`, el emparejamiento `-Z unstable-options`, y el requisito de ≥1 `--print`. El argv lo genera rust-analyzer (no el peer), pero verifica el allowlist como si fuera la última línea: ¿hay un `--print` que compile (p.ej. `native-static-libs`, `link-args`)? ¿un valor `KIND=PATH`? ¿una ruta `.json` como `--target`? ¿un `-o`/`--emit`/`-L`/`--extern`/`-C`? ¿un token que consuma el siguiente flag como valor y desactive una comprobación?

Verifica también: (a) que las cuatro formas previamente admitidas siguen admitidas; (b) que TODOS los `refused` previos siguen refused (en especial `rustc --crate-name fixture /source/src/lib.rs`); (c) que los brazos `rust-analyzer` y `cargo` NO cambiaron; (d) que el corte `m6-03` sigue siendo una aserción de allowlist cerrado (no se debilitó a un check de basename).

## Lee

`crates/execution-adapter/src/analyzer_native.rs` (helper `rustc_is_readonly_probe`, `plain_word`, `STOPPING_PRINT_KINDS`, brazo `admitted_argv`, el corte `m6_initialize_spawns_only_the_expected_guest_programs`, y el unit test `the_guest_argv_allowlist_admits_the_expected_forms_and_nothing_else`); `docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md` §7 y su enmienda 2026-09-12; el reporte del worker en `docs/validation/M6/delegation/W12-guest-argv-print-probe/report.md`. El diff exacto: `git diff -- crates/execution-adapter/src/analyzer_native.rs`.

## Entrega

Escribe tu veredicto directamente y deja claro el conteo de findings. Formato: modelo/versión/effort; archivos+hash revisados; findings P0–P3 por caso (cada uno con el argv concreto que pasaría y por qué es peligroso, si lo hubiera); veredicto **Approve / Approve-with-P3 / Block**; y una frase sobre si el allowlist quedó estrictamente igual o más conservador que antes salvo por la sonda de solo lectura añadida. Un P0/P1 (admite algo que compila/escribe/lee proyecto) BLOQUEA.
