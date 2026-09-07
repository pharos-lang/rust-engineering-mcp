Task F01 completada sin commits ni cambios fuera de ownership.

Result:
- 9 fixtures nextest aisladas con `Cargo.toml`, `Cargo.lock` manual y código.
- 12 muestras hostiles JUnit/JSON/HTML/tar deterministas.
- Matriz M3 skeleton con 19 filas `Pending`.
- ADR-060..063 aceptados según texto solicitado.
- Índice ADR actualizado.
- `scripts/test-fixtures.py` no se modificó: no requiere inventario para estas fixtures.

Tests/evidence:
- `rustfmt --edition 2024 fixtures/nextest/*/src/lib.rs`: exit 0.
- `python3 -B scripts/check-architecture.py`: exit 0.
- Validación estructural, hashes y tars: exit 0; 9 crates, 12 artefactos.
- Cargo check/clippy/test no ejecutados deliberadamente: el paquete exige no ejecutar Cargo host contra fixtures hostiles.
- No se ejecutó `scripts/test-fixtures.py`, al no modificar su corpus inventory.

Archivos principales:

- [fixtures/nextest/README.md](/Users/cburgosro/Projects/rust-mcp/fixtures/nextest/README.md)
- [fixtures/hostile-reports/README.md](/Users/cburgosro/Projects/rust-mcp/fixtures/hostile-reports/README.md)
- [M3-matrix.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-matrix.md)
- [ADR index](/Users/cburgosro/Projects/rust-mcp/docs/adr/README.md)

SHA-256: todos los archivos nuevos/cambiados bajo ownership fueron verificados; los hashes están registrados en `fixtures/hostile-reports/README.md` para los artefactos hostiles.

Riesgos:
- Las fixtures requieren ejecución posterior dentro del guest/Docker autorizado.
- La calificación de runtime, gateway, Tasks y provisioning sigue pendiente.
- El checkout contiene cambios concurrentes ajenos, preservados sin modificación.

Needs para otros especialistas:
- Ejecutar la matriz nextest/Docker y validar límites de output, leak, cancelación y cleanup.
- Calificar parsers/egress contra `fixtures/hostile-reports/`.
- Registrar evidencia final en `docs/validation/M3-matrix.md`.