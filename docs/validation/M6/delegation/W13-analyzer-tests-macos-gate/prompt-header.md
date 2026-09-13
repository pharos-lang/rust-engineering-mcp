# W13 — completar el gate macOS de los tests con peer `/bin/sh` (portabilidad CI)

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker sobre código de test en 3 archivos. Orquestador: Claude Fable 5.1 (como Opus 4.8). No puedes lanzar subagentes. **Nunca corras en segundo plano. No corras Docker. No hagas commit.**

## Diagnóstico (vinculante — reproducido por el orquestador contra el CI del PR)

El PR de M6 falla en Windows y Linux (pasa en macOS). Los tests que usan un peer `/bin/sh` scripted (`shell()`, `printf`/`cat`/`sleep`/`/dev/null`) solo funcionan en macOS: en Linux fallan en runtime con `Error: "Unavailable"` y `#[cfg(unix)]` NO sirve. La mayoría de esos tests YA están `#[cfg(target_os = "macos")]`, pero:

1. **`crates/execution-adapter/src/lsp_session.rs`**: 15 tests están macOS-gated y 1 (`every_session_error_is_terminal_and_named`) es portable. Los *helpers* de test están SIN gate → en Linux/Windows quedan sin usar y `cargo clippy -- -D warnings` los vuelve error. Items a gatear (exactamente los que el CI reporta sin usar en Linux): el `use rust_engineering_application::NeverCancel;` del `mod tests`; el `type Failure`; las funciones `peer`, `shell`, `frame_literal`, `budget`, `notification`, `status_record`; el struct `CancelAfter`; y los dos métodos `#[cfg(test)]` `stderr_evidence` y `server_requests` (líneas ~847/856, están FUERA del `mod tests`, en un impl del tipo de producción, gatéalos `#[cfg(all(test, target_os = "macos"))]`). NO gatees `every_session_error_is_terminal_and_named` ni nada que ese test use.
2. **`crates/execution-adapter/src/analyzer_gateway.rs`**: los dos tests `answer_references_marks_the_declaration_from_a_real_two_response_peer` y `answer_references_times_out_on_a_silent_second_request` NO tienen gate y usan `shell()`/`frame_literal()`; en Linux corren y fallan, en Windows no hay `/bin/sh`. Añádeles `#[cfg(target_os = "macos")]`. Tras eso, `shell()`/`frame_literal()` de ESTE archivo quedarán sin usar en no-macOS (sus únicos usuarios son esos 2 tests) → gatéalos `#[cfg(target_os = "macos")]` también. NO toques los tests de lógica pura (`declarations_are_exactly_...`, `cap_raw_locations`, etc.).
3. **`crates/mcp-server/src/stdio/mutation/analyzer_action.rs`**: 3 tests macOS-gated + 10 portables. Los helpers usados solo por los 3 macOS-gated quedan sin usar en Linux. Items a gatear `#[cfg(target_os = "macos")]` (exactamente los que el CI reporta): imports `AnalyzerActionValidationMethod`, `MutationCandidate`, `SourceFile`; funciones `answer_action`, `preview_request`, `run_preview`, `range`, `execution_hash`; struct/impl `Port`, `Writer`, `Proceed`, `Project` (incluida su impl con `new`/`path`/`source`/`registry`); const `SOURCE`; y `resolving` (associated fn). NO gatees ningún helper que un test portable use — si dudas de uno, verifícalo por búsqueda antes de gatear.

Usa **`#[cfg(target_os = "macos")]`** (no `unix`): en Linux los tests fallan de verdad. No cambies lógica de producto ni de los tests; solo añades atributos `cfg`. No muevas código.

## Verificación (foreground)

Oráculo principal (reproduce el fallo de Linux localmente; el target ya está instalado):
```text
CARGO_TARGET_DIR=target/linux-repro cargo clippy -p rust-engineering-execution --all-targets --locked --offline --target x86_64-unknown-linux-gnu -- -D warnings
```
DEBE terminar **sin errores**. Itera: si al gatear un item aparece OTRO sin usar (cascada), gatéalo también con el mismo `cfg` y vuelve a correr, hasta limpio.

macOS (que todo siga compilando y los tests macOS sigan presentes):
```text
cargo clippy -p rust-engineering-execution -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-execution --lib --locked --offline
```
Ambos verdes.

**mcp-server para Linux NO se puede verificar localmente** (falta `x86_64-linux-gnu-gcc` para zstd/sqlite/ring). Para `analyzer_action.rs`, gatea exactamente los items de la lista del CI de arriba y confirma que el clippy macОS sigue limpio; el orquestador confirma con el CI.

Reporta: Task / Result / Files changed / La lista exacta de items gateados por archivo / Salida del clippy Linux de execution-adapter (limpio) y del clippy macOS / Cascadas encontradas / Risks / Open issues. No commit.
