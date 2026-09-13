# W14 — tests unitarios portables para subir la cobertura de código nuevo del analyzer ≥80% (SonarCloud)

Modelo solicitado: Claude Opus 5 (`claude -p --model opus --effort high`). Rol: worker de implementación (tests). Orquestador: Claude Fable 5.1 (como Opus 4.8). No puedes lanzar subagentes. **Nunca corras en segundo plano. No corras Docker. No hagas commit.**

## Objetivo

SonarCloud rechaza el PR de M6: `new_coverage` = **78.1% < 80%**. El repo PROHÍBE excluir `crates/**/*.rs` de la cobertura (`test-gate-reporting.py`). El hueco es el código de integración del analyzer cuyos tests son macOS/Docker-gated, así que en el llvm-cov de Linux (donde SonarCloud mide) queda sin cubrir. **Sube la cobertura escribiendo tests unitarios PORTABLES** (que compilen y corran en todos los targets: **sin `#[cfg(target_os = ...)]`, sin peer `/bin/sh`, sin Docker, sin `#[ignore]`**) para las **funciones puras** de:

- `crates/execution-adapter/src/analyzer_gateway.rs`
- `crates/mcp-server/src/stdio/mutation/analyzer_action.rs`

Necesitas cubrir **≥300 líneas** hoy sin cubrir (el gate necesita ~200; deja margen).

## Funciones objetivo (puras, testeables sin gateway/Docker)

**`analyzer_gateway.rs`** (líneas sin cubrir incluyen 328-345, 353-408, 421-508, 631-759): `refusal`, `stamp`, `absent_session`, `fingerprint`, `analyzer_configuration`, `document`, `wire_positions`, `snapshot`, `file_uri`, `runtime_identity`/`malformed`, `classify` (mapea `SessionError`+`Stage`→`AnalyzerFailure`, cubre todas las combinaciones), y la parte pura de `protocol`. **NO** intentes `execute`/`execute_bounded`/`converse`/`guest_verdict` (necesitan el gateway/sesión Docker) — déjalas.

**`analyzer_action.rs`** (sin cubrir 230-306, 335-534, 579-618, 781-808): los mapeos de error y builders puros: `Event::event`/`message` (todas las variantes), `Failure::with`, `From<ApplyCode> for Failure`, `ApplyOutput::status`/`event`/`response_lost`/`new`/`failure`, `bootstrap_refusal`, `admitted`, `preview_output`, `joined_output`, `validate_preview_size` (dentro y fuera del límite), `receipt_data`, `lock_failure`, `worker_failure` (todas las fases/errores), `mutation_failure` (todas las variantes de `MutationError`). **NO** intentes `preview`/`commit`/`receipt` (el `Ports` impl que necesita store/writer) salvo que exista un doble portable ya en el módulo.

Reutiliza los helpers de test existentes (`bundle`, `snapshot`, `document`, `location_json` en `analyzer_gateway.rs`; los fixtures del módulo de test de `analyzer_action.rs`). Añade los tests en los `#[cfg(test)] mod tests` existentes, como `#[test]` normales (sin gate de plataforma). Cubre cada rama de los `match` (cada `error_code`/`status`/`AnalyzerFailure`/fase) para maximizar líneas.

## Verificación (foreground, macOS nativo)

`cargo llvm-cov` está instalado. Mide la cobertura AISLADA de tus tests nuevos (corriéndolos solos) para confirmar que tocan las líneas antes sin cubrir:

```text
cargo llvm-cov -p rust-engineering-execution --lib --locked --offline --summary-only -- <tus_tests_nuevos_de_gateway>
cargo llvm-cov -p rust-engineering-mcp --lib --locked --offline --summary-only -- <tus_tests_nuevos_de_action>
```

y confirma que `analyzer_gateway.rs` / `analyzer_action.rs` suben su % de líneas cubiertas por SOLO esos tests. También corre los tests completos para que pasen:

```text
cargo test -p rust-engineering-execution --lib --locked --offline analyzer_gateway
cargo test -p rust-engineering-mcp --lib --locked --offline analyzer_action
cargo fmt --all -- --check
cargo clippy -p rust-engineering-execution -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings
```

Todos verdes. Como los tests son PORTABLES, correrán también en el llvm-cov de Linux del CI y añadirán la misma cobertura. Reporta: Task / Result / Files changed / Lista de tests añadidos por función cubierta / Estimación de líneas nuevas cubiertas por archivo (del `--summary-only` aislado) / Salida de fmt/clippy/test / Risks / Open issues. No commit.
