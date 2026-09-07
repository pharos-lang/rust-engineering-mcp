Task F02 completada.

- Creados 6 workspaces nombrados en `fixtures/mutation/**`, más `canary.txt`.
- Añadidos oráculos, códigos hipotéticos y requisitos de containment.
- Documentación: [M3-05-fixtures.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-05-fixtures.md)
  - SHA-256: `2163b576000c5d91ad30ff8458ce5bfe7810ce886e0574988c899caf8e188385`
- Inventario SHA-256 completo de los 26 archivos incluido en la documentación.

Checks:

- `rustfmt --edition 2024 --check`: exit 0
- `tomllib` structural sanity: exit 0
- `git diff --check`: exit 0
- No se ejecutaron Cargo, cargo-mutants, Docker ni fixtures en el host.

Riesgo abierto: la solicitud menciona “siete workspaces”, pero enumera seis. Se implementaron exactamente las seis filas nombradas; I05/orquestador debe confirmar si requiere un séptimo fixture.