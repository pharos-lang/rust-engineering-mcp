# W39 — cobertura portable (Linux llvm-cov) del código Rust nuevo de M8: `doctor.rs`, `mutation_cli.rs`, `host_config.rs`

Worker: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Sin subagentes, sin background, sin Docker, sin commit.

## Hallazgo clave: por qué la cobertura de las pruebas CLI existentes era 0

`crates/mcp-server/tests/cli.rs` y `tests/doctor.rs` ya tenían pruebas extensas para
`--rustsec-snapshot`/`--catalog-*` dentro/fuera de un root, `mutation list` sobre un
`state_root` nunca inicializado o inexistente, etc. Pero su helper `run()` invoca el
binario compilado con **`Command::new(...).env_clear()`**, que borra `LLVM_PROFILE_FILE`
antes de lanzar el subproceso. `cargo-llvm-cov` propaga la cobertura de subprocesos vía
esa variable de entorno, así que **ninguna prueba que use `env_clear()` en su spawn
aporta cobertura al binario probado**, sin importar la plataforma. Lo verifiqué
comparando `cargo llvm-cov -p rust-engineering-mcp --tests` (con las 61 pruebas de
integración incluidas) contra `--bins` (solo las pruebas unitarias in-process del
binario): los tres archivos objetivo dan **exactamente el mismo** conteo de líneas
cubiertas en ambos casos. Esto confirma que la única vía real de cobertura para estos
archivos son pruebas unitarias in-process (`#[cfg(test)] mod tests` dentro del propio
binario), y que en Linux/SonarCloud (donde `tests/doctor.rs` además está excluido por
`#![cfg(target_os = "macos")]`) el resultado es el mismo problema, no uno adicional.

Confirmé además que en cualquier plataforma que no sea macOS,
`NativeMutationStore::open` (`crates/project-adapter/src/mutation_store.rs`, rama
`#[cfg(not(target_os = "macos"))]`) devuelve siempre `Err(UnsupportedPlatform)`. Esto
hace **inalcanzables en Linux** las ramas `Ok(records)`, `Err(Busy)` y
`Err(RecoveryRequired)` de la clasificación de `doctor.mutation_journals` mediante
cualquier prueba que pase por el store real, sea vía CLI o vía `inspect()` con un
directorio de journal real — de ahí la instrucción explícita del encargo de extraer
funciones puras y probarlas con registros sintéticos.

## Cambios (solo `#[cfg(test)] mod tests` y refactors puros mínimos)

### `doctor.rs` (+ `doctor/tests.rs`)
- Extraje `classify_mutation_records(Result<Vec<MutationRecordSummary>, MutationError>) -> MutationJournalsReport`
  de `mutation_journals`, pura, sin I/O ni store. `mutation_journals` queda como wrapper
  delgado (comprobación de directorio + llamada a la función pura).
- 9 pruebas nuevas en `doctor/tests.rs`: vacío sin bloqueo; conteo pending/terminal por
  kind conocido; kinds desconocidos a `0.3.0` (`analyzer_action_apply`, `dependency_add`,
  `dependency_remove`) bloquean incluso en fase terminal; rama `Busy`; rama
  `RecoveryRequired` (unknown_format=1, dos notas); fallback genérico para otros errores
  (`Io`, `UnsupportedPlatform`, `PermissionDenied`); `mutation_journals()` con directorio
  inexistente (portable, solo `std::fs`); línea humana `human()` con
  `mutation_journals: pending=… terminal=… …` poblado, y el caso `mutation_journals: null`
  (campo `None` tras `Report::new`, sin la línea en `human()`).

### `mutation_cli.rs`
- Extraje `build_report(action, Result<(Vec<Record>, Option<bool>), MutationError>) -> (Report, u8)`
  y `render(&Report, json: bool) -> Option<Vec<u8>>` de `run()`; `run()` queda reducido a
  orquestar `execute` → `build_report` → `render` → escritura tokio/stdout.
- 13 pruebas nuevas: `journal_dir_exists` directa (store no inicializado → `Ok(false)`;
  `state_root` inexistente → `Err(NotFound)`; ruta ocupada por un archivo → `Err(Io)`) con
  `std::env::temp_dir()` + limpieza manual, igual que ya hace `tests/cli.rs` (sin añadir
  `tempfile` como dependencia nueva); `execute()` directa para el mismo par de casos sin
  abrir el store macOS; `build_report` para las 4 combinaciones de mensaje/status/código;
  `error_code` y `state` exhaustivas sobre todas las variantes; `render` con
  round-trip JSON, formato humano con registros, y el límite de 128 KiB.

### `host_config.rs`
- 2 módulos de test nuevos (`audit_tests`, `catalog_tests`), mismo patrón que
  `security_tests` ya existente: rutas sintéticas (no requieren existir en disco, el
  parser solo compara prefijos) para `--rustsec-snapshot`/`--rustsec-sha256` y
  `--catalog-store`/`--catalog-trust`/`--catalog-model-dir`/`--catalog-index-store`
  dentro/fuera de un `--root`, más las combinaciones de emparejamiento cerrado
  (`index_store` sin `model_dir` → rechazado; store/trust solos aceptados).

`contract_cli.rs` y `main.rs` (mencionados en el contexto de huecos de cobertura) quedan
**fuera de alcance**: no están en la lista de "Archivos permitidos" del encargo.

## Cobertura antes/después (`cargo llvm-cov -p rust-engineering-mcp --bins`, macOS;
ver nota arriba: idéntica a `--tests` para estos 3 archivos, sirve de proxy fiable de lo
que medirá SonarCloud en Linux)

| Archivo | Líneas antes | Cobertura antes | Líneas después | Cobertura después |
|---|---|---|---|---|
| `doctor.rs` | 507 (203 sin cubrir) | 59.96% | 513 (126 sin cubrir) | 75.44% |
| `mutation_cli.rs` | 182 (114 sin cubrir) | 37.36% | 349 (45 sin cubrir) | 87.11% |
| `host_config.rs` | 526 (122 sin cubrir) | 76.81% | 630 (112 sin cubrir) | 82.22% |

Estas cifras son cobertura de archivo completo (no el diff de líneas nuevas que mide
SonarCloud), pero como demostrado arriba, en estos tres archivos toda la cobertura real
proviene de pruebas unitarias in-process portables — la métrica de SonarCloud sobre
código nuevo debería moverse en la misma dirección. Las ramas descritas en el encargo
(clasificación de `mutation_journals`, `store_initialized`/directorio inexistente,
contención de `--rustsec-snapshot`/`--catalog-*`) están ahora cubiertas por pruebas que
no dependen de `#[cfg(target_os = "macos")]`, Docker, ni el store nativo.

## Verificación

- `cargo fmt --all -- --check` — limpio.
- `cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings`
  — limpio (incluye `clippy::unwrap_used`/`expect_used`/`panic` denegados a nivel de
  workspace; las pruebas nuevas usan `?` con `Result<(), …>` en vez de `.expect()`).
- `cargo test -p rust-engineering-mcp --locked --offline` — 479 passed, 0 failed, 3
  ignored (unittests del binario) + 61 (`protocol.rs`) + 23 (`cli.rs`) + 10 (`doctor.rs`)
  + el resto de binarios de integración, todos en verde.
- `git status --short crates/mcp-server/tests/snapshots` — sin cambios (vacío).
- Ningún archivo fuera de la lista permitida fue modificado.

## Archivos tocados

- `crates/mcp-server/src/doctor.rs` (refactor puro mínimo: extracción de
  `classify_mutation_records`)
- `crates/mcp-server/src/doctor/tests.rs` (+9 pruebas)
- `crates/mcp-server/src/mutation_cli.rs` (refactor puro mínimo: extracción de
  `build_report`/`render`; +13 pruebas)
- `crates/mcp-server/src/host_config.rs` (+6 pruebas en 2 módulos nuevos)

No hubo commit, conforme al encargo.
