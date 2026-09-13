# W13 — gate macOS de los tests con peer `/bin/sh` (portabilidad CI Windows/Linux)

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort high --disallowedTools Agent Task`) |
| Inicio / fin (UTC) | 2026-09-13T16:37:25Z / 2026-09-13T16:48:10Z |
| Resultado | Ediciones aplicadas; el worker NO pudo correr el clippy Linux (su allowlist no cubría el prefijo `CARGO_TARGET_DIR=… cargo clippy`) y quedó esperando verificación |

## Origen

El PR de M6 falló en CI: `portable` (Windows+Linux) por `-D warnings` sobre helpers de test sin usar, y `SonarCloud` (llvm-cov Linux) porque los dos tests `answer_references_*` (sin gate) corren en Linux y fallan (`Error: "Unavailable"` — el peer `/bin/sh` solo funciona en macOS). macOS pasó. Primera corrida no-macOS de M6.

## Cambios (solo atributos `#[cfg(target_os = "macos")]`, sin lógica)

- `crates/execution-adapter/src/lsp_session.rs`: gate de helpers/imports huérfanos (`NeverCancel`, `Failure`, `peer`, `shell`, `frame_literal`, `budget`, `notification`, `status_record`, `CancelAfter`) y de los métodos `#[cfg(test)]` `stderr_evidence`/`server_requests` → `#[cfg(all(test, target_os = "macos"))]`. El test portable `every_session_error_is_terminal_and_named` intacto.
- `crates/execution-adapter/src/analyzer_gateway.rs`: gate de los 2 tests `answer_references_*` + `shell`/`frame_literal`. **Cascada detectada y cerrada por el orquestador** (el worker no pudo correr el oráculo): `location_json`, `references_response` y el `use NeverCancel` de ese módulo también quedaron huérfanos → gateados por el orquestador tras verificar con el clippy Linux.
- `crates/mcp-server/src/stdio/mutation/analyzer_action.rs`: gate de los 14 items huérfanos del CI (`AnalyzerActionValidationMethod`, `MutationCandidate`, `SourceFile`, `answer_action`, `preview_request`, `run_preview`, `range`, `execution_hash`, structs `Port`/`Writer`/`Proceed`/`Project` con sus impls, `resolving`, `SOURCE`).
- `sonar-project.properties` (orquestador): `analyzer_native.rs` añadido a `sonar.test.inclusions` (como `performance_native`/`security_native`) para que Sonar no exija cobertura del módulo de calibración `#[ignore]`.

## Verificación (orquestador)

- `cargo clippy -p rust-engineering-execution --all-targets --target x86_64-unknown-linux-gnu -- -D warnings` → **limpio** (reproduce y cierra el fallo de Linux; 10→3→0 errores tras la cascada).
- `cargo clippy -p rust-engineering-execution -p rust-engineering-mcp --all-targets -- -D warnings` (macOS) → **limpio** (los tests macOS siguen compilando).
- mcp-server para Linux no verificable localmente (falta `x86_64-linux-gnu-gcc`); se confía en la lista exacta del CI + clippy macOS + la re-corrida de CI.

## Decisión

`#[cfg(target_os = "macos")]` (no `unix`): los tests fallan de verdad en Linux (`Unavailable`). La cobertura de lógica pura del analyzer (tests portables de `analyzer_gateway`) permanece en Linux; la variante de integración con peer es macOS-only, coherente con el host calificado.
