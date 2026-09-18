# W14 — M8-03: preflight pasivo `doctor.mutation_journals` + fixture de permisos revocados

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker de implementación (Rust). Orquestador: Claude Fable 5.1. Sin subagentes. **Nunca en segundo plano** (cargo en primer plano; espera). No commit. No Docker. **Archivos permitidos:** `crates/mcp-server/src/doctor*.rs` (y el módulo donde viva el informe de `doctor`), `crates/mcp-server/tests/doctor.rs`, `crates/mcp-server/tests/snapshots/doctor-report.json` (si el schema cambia, regenéralo por el mecanismo del repo), `crates/project-adapter/tests/support/native_mutation.rs` o el archivo de tests nativos M2 donde encaje el nuevo fixture, `docs/tools.md` (solo la sección de `doctor`). No toques `Cargo.*`, `scripts/`, snapshots de tools.

## Contexto y decisión (D12 §3 y §8 en `docs/validation/M8/03.md`)

Lee `docs/validation/M8/03.md`, `docs/validation/M8/03-formats-analysis.md` §2 (journal M2: fases, `decode_envelope`, `RecoveryRequired`, `mutation list`) y el código de `doctor` (`crates/mcp-server/src/doctor*.rs`, `tests/doctor.rs`, snapshot `doctor-report.json`, `format_version 1`). `doctor` es pasivo: diagnostica estado configurado sin ejecutar proyecto.

## Tareas

1. **`doctor` → sección `mutation_journals`** (aditiva, sin cambiar campos existentes; `format_version` se mantiene si el cambio es aditivo — documenta en el JSON del informe que es aditivo desde 0.8.0): cuando `--state-root` está configurado, lista los journals M2 con la misma lectura que `mutation list` y resume: `pending` (no terminales) por fase y por `operation_kind`, `terminal`, `unknown_format` (envelope/kind que este binario no interpreta → fail-closed por registro, `RecoveryRequired`), y `downgrade_blocked: bool` (= hay pendientes o desconocidos; explica en `notes[]` que un binario anterior no podrá interpretar kinds nuevos, p. ej. `analyzer_action_apply`). Sin `--state-root`: `mutation_journals: null` o `{"configured": false}` (elige lo coherente con el resto del informe). Salida humana: una línea resumen. Nunca leer contenido del workspace ni source; solo metadatos del journal. Sin `unwrap` en rutas normales.
2. **Tests** en `tests/doctor.rs`: (a) store vacío → 0/0/0, `downgrade_blocked: false`; (b) store con un journal pendiente (usa los helpers de escritura de journal que ya usan los tests M2 — `crates/project-adapter/tests/support/native_mutation.rs` o el API público del `MutationStore`; si solo es alcanzable con `--features test-hooks`, úsalo y márcalo como en los tests vecinos) → `pending ≥ 1`, `downgrade_blocked: true`; (c) journal con `operation_kind` desconocido (bytes fabricados con el envelope real y un kind inexistente) → `unknown_format ≥ 1`, sin pánico. Actualiza `doctor-report.json` si el snapshot cubre el informe.
3. **Fixture «permisos revocados a mitad de operación»** (D12 §8): en el test nativo M2 apropiado (`native_mutation.rs`/tests del writer), entre `preview` y `commit` revoca permisos de escritura del directorio destino (`chmod 0o500`) y confirma fail-closed (`PermissionDenied`/`Io` clasificado, sin bytes parciales, journal en fase no terminal recuperable, restauración de permisos al final). Si el test requiere Docker/imagen, hazlo `#[ignore]` y nativo como sus vecinos, y déjalo listo para el driver M2 (`scripts/test-m2-runtime.py`); si es puro filesystem, portable con `#[cfg(unix)]`.

## Verificación (foreground)

```text
cargo fmt --all -- --check
cargo clippy -p rust-engineering-mcp -p rust-engineering-project --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-mcp --locked --offline --test doctor
cargo test -p rust-engineering-project --locked --offline <nombre del test nuevo>   # o --ignored si es nativo
git status --short crates/mcp-server/tests/snapshots   # solo doctor-report.json puede cambiar
```
Escribe el informe (Task / Result / Files changed / Tests / Salida / Risks / Open issues) en `docs/validation/M8/delegation/W14-doctor-journals/report.md` y en tu última respuesta. No commit.
