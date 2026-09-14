# W03 — disposición del orquestador (2026-09-14)

Veredicto: **aceptado**. Diff revisado por el orquestador: `main.rs:59` lista las
36 tools en el orden del `tools/list` live y el paréntesis sigue siendo exacto
(runtime M6 vía `--rust-image`; grant `--allow-analyzer-action-write` para
`action.apply`); `docs/tools.md` separa release `0.3.0` (31) de checkout (36);
`architecture.md` 291/333 → 36; índice ADR-078/079/081/085 con el formato de la
casa; `m2-m8.md:12` con el estado real. Frases históricas conservadas con
justificación por sitio (README ya distinguía release/checkout;
`client-configuration.md` 243/316/418 describen M5 o el comportamiento vigente
con imagen M5). Verificación del worker: fmt limpio, clippy `-D warnings`
verde (11 m 48 s en frío), `cli` 13/13, links-check 0 rotos, `--help` contiene
`rust.analyzer.action.apply`.

Sin findings. El cambio de `crates/` (literal de `--help`) obliga a un gate
`core` sobre los bytes finales antes de cerrar M8-02 (G5); para el commit de
M8-01 basta la verificación focalizada del worker (fmt/clippy/cli), coherente
con la regla del owner sobre cambios de texto sin lógica.
