# W14 — disposición del orquestador (2026-09-15)

Veredicto: **aceptado tras verificación del orquestador y corrección W14b**. El
worker no pudo ejecutar `cargo` en su sesión (denegación de permisos del host)
y lo declaró honestamente. Verificación del orquestador sobre sus bytes: `cargo
fmt` limpio; `cargo clippy -p rust-engineering-mcp -p rust-engineering-project
--all-targets -D warnings` limpio; `doctor` 4/5 → el test del kind desconocido
fallaba por dos defectos del **fixture** (proyecto sin target de crate; patch
de manifiesto con cambio de `package.version`, inválido por
`validate_manifest_patch`), no del producto — corregidos en W14b → **5/5**. El
fixture de permisos revocados (`revoked_destination_permissions_leave_a_recoverable_non_terminal_journal`,
`#[cfg(test)]` del lib de `project-adapter`) **pasa** (1/1). Snapshot
`doctor-report.json` es el único snapshot tocado (`mutation_journals: null`).
`doctor.mutation_journals` cierra D12 §3 (preflight pasivo de downgrade).
