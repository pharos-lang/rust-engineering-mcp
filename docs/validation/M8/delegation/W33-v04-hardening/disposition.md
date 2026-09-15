# W33 — disposición del orquestador (2026-09-15)

Veredicto: **aceptado**. `persist-credentials: false` en los 5 checkouts de los 4 workflows (verificado por grep; ningún paso posterior usa credenciales persistidas); `host_config.rs` rechaza `--rustsec-snapshot` y `--catalog-*` dentro de roots con el patrón existente; tests en `cli.rs`; fmt limpio; clippy/tests en el gate final. Cierra V04 F-05 (parcial, RR-10 ampliado) y F-06.
