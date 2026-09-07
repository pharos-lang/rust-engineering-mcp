Bloqueado: dejé el corte declarado como `Unavailable`, no como una falsa implementación ejecutable.

Entregado parcialmente:

- Contratos `coverage` de dominio/aplicación con regla de denominador cero.
- Parser JSON LLVM acotado, con dedupe de archivos compartidos.
- Variantes cerradas de `RustCommand` y argv de `cargo llvm-cov --no-report` + tres `report`.
- Tool 20 `rust.coverage`, schema y snapshot; responde fail-closed.
- Fixtures y pruebas Docker ignoradas.
- Documentación y matriz M3 actualizadas como `Blocked`.

Bloqueador real: el egress requerido en `/work/coverage` exige ampliar el verificador aplicado de mounts Docker. Ese archivo está fuera de la frontera autorizada para este paquete; sin esa verificación, habilitar ejecución/publicación de JSON/LCOV/HTML sería inseguro. Además, hubo escrituras concurrentes en `RustCommand`/`Phase`, impidiendo un gate final estable.

Validaciones realizadas:

- `coverage_json`: 2/2 passed.
- `domain coverage`: 2/2 passed.
- `application coverage`: 1/1 passed.
- Protocolo M3-01 con tool 20 registrado: 1/1 passed.
- `git diff --check`: passed.
- Docker: no ejecutado; pruebas marcadas `#[ignore]`.

SHA-256 principal:

- `coverage.rs` dominio: `2372655197b27c1deaf5577b7dfbf353cede6c018cef6789790dc94dde07fad5`
- Parser: `e7dc826d168d454c003922789a9abe4d77dea5fc5ed0390fe769517a6a0944e9`
- Tool MCP: `f8fa3c52e17c2948b07eb874b7b10354432b115186242bb3fcd10b7e69bab016`
- Snapshot: `7ffc7f6c0b88559c64173531045c8d44857ebde761fd128a8dafb0fe78a9962b`