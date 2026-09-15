# W14b — arreglar `mutation_journals_reports_an_unrecognized_kind_as_unknown_format_without_panicking`

## Task

W14 dejó un test nuevo en `crates/mcp-server/tests/doctor.rs` que no compilaba/pasaba en su
sesión. El orquestador confirmó: `cargo fmt` limpio, `cargo clippy -p rust-engineering-mcp
-p rust-engineering-project --all-targets -D warnings` limpio, pero
`cargo test -p rust-engineering-mcp --test doctor` daba 4/5, con el test de arriba fallando
con `Error: "Rejected(InvalidProject)"`. Se pidió diagnosticar y arreglar el test (o el
producto si la decisión D12 §3 lo contradice), sin commit, sin tocar más archivos que los
listados.

## Causa raíz (dos defectos encadenados en el fixture del test, no en el producto)

1. **Proyecto sin target de crate.** El fixture del test solo escribía `Cargo.toml` (sin
   `src/lib.rs`/`src/main.rs`). `manifest::validate` en
   `crates/project-adapter/src/manifest.rs:335-358` exige que se encuentre al menos un
   target (`lib` o `bin`); si no, devuelve `ProjectError::Rejected(InvalidProject)` **antes**
   de que el test llegue a ejercitar nada del store de mutaciones o de `doctor`. Este era el
   error original reportado por W14.
2. **Manifest patch semánticamente inválido.** Una vez añadido `src/lib.rs`, el test seguía
   fallando, ahora con `Error: "Invalid"` (`MutationError::Invalid`). El test construía el
   candidato de mutación cambiando `version = "0.1.0"` → `"0.1.1"` en `Cargo.toml`.
   `validate_manifest_patch` (`crates/project-adapter/src/semantic_delta.rs:13-44`) solo
   permite diffs dentro de `lints`/`features`/`profile`/`workspace.dependencies`/
   `workspace.lints`; cualquier otro campo del manifiesto (incluida `package.version`) debe
   quedar bit-a-bit idéntico entre `before` y `after`, o se rechaza como `Invalid`. Un bump
   de versión desnudo nunca fue un `ManifestPatch` válido. El fixture de referencia ya
   existente en `crates/project-adapter/tests/support/native_mutation.rs` evita justamente
   este problema mutando `[lints.rust] unsafe_code = "forbid"` en vez de la versión.

Ninguno de los dos defectos está en el comportamiento de `doctor`/D12 §3: una vez que el
`commit` real tiene éxito y el campo `operation` del journal se corrompe a un valor
desconocido, `doctor` sí clasifica correctamente eso como `unknown_format >= 1` y
`downgrade_blocked: true` (vía `MutationError::RecoveryRequired` fail-closed en
`operation_kind`, `crates/project-adapter/src/filesystem/macos/mutation.rs:926-936`, propagado
por `mutation_journals` en `crates/mcp-server/src/doctor.rs:186-238`). No hizo falta tocar
producto.

## Files changed

- `crates/mcp-server/tests/doctor.rs` (único archivo modificado):
  - El fixture del proyecto ahora crea `src/lib.rs` además de `Cargo.toml`, para que
    `SecureProjects::open` acepte el proyecto.
  - La construcción de `after` solo reescribe el contenido de `Cargo.toml` (dejando
    `src/lib.rs` intacto), y el cambio en `Cargo.toml` pasa de un bump de versión a añadir
    `[lints.rust]\nunsafe_code = "forbid"`, que es el único tipo de diff que
    `validate_manifest_patch` acepta para `MutationKind::ManifestPatch`, igual que el
    fixture de referencia en `crates/project-adapter/tests/support/native_mutation.rs`.

No se tocó `crates/mcp-server/src/doctor.rs` ni
`crates/project-adapter/tests/support/native_mutation.rs`.

## Salidas

```
$ cargo test -p rust-engineering-mcp --locked --offline --test doctor
running 5 tests
test mutation_journals_reports_an_unrecognized_kind_as_unknown_format_without_panicking ... ok
test mutation_journals_is_null_without_state_root_and_empty_with_no_journals ... ok
test passive_doctor_and_version_are_bounded_and_never_execute_path_tools ... ok
test doctor_reads_signed_catalog_without_admin_lease_or_staging_mutation ... ok
test doctor_rejects_closed_cli_syntax_and_reports_configured_access_failures ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.50s

$ cargo fmt --all -- --check
(sin salida — limpio)

$ cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 02s
(sin warnings)

$ git status --short crates/mcp-server/tests/snapshots
 M crates/mcp-server/tests/snapshots/doctor-report.json
```

Solo `doctor-report.json` aparece modificado bajo `tests/snapshots` (preexistente al inicio
de esta sesión, no tocado por este fix). No se hizo commit.
