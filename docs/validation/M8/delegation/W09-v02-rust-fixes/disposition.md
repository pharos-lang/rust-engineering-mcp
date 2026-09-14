# W09 — disposición del orquestador (2026-09-14)

Veredicto: **aceptado**. Verificado por el orquestador: `contract --json` con
exactamente las 14 tools `executes_project_code: true` fijadas en la
disposición V02; `benchmark.compare` `requires_runtime: none`, analyzer
`analyzer`; 0 discrepancias de hash frente a los snapshots y frente al
manifiesto `freeze-0.8.0.json` (el manifiesto no cambia: `executes_project_code`
no forma parte de él); snapshots sin cambios nuevos (solo los 5 `analyzer-*`
de W05); `cargo fmt` limpio. Tests de valores, de literales por igualdad de
strings y de annotations en `cli.rs` añadidos; stderr en fallo. Clippy y la
suite completa se cubren en el gate `core` sobre bytes finales.
