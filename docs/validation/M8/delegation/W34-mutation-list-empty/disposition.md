# W34 — disposición del orquestador (2026-09-15)

Veredicto: **aceptado**. Causa raíz: `mutation_cli.rs` abría el directorio de journals sin comprobar su existencia (`ENOENT` → catch-all `Io`); ahora comprueba como `doctor` y responde `passed`, `store_initialized: false`, `count: 0` (verificado por el orquestador sobre un directorio vacío: exit 0). Sin cambios en el store ni en snapshots; fmt limpio; tests `cli` en el gate final. Cierra F-2 de la reproducción por tercero.
