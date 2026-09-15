# W14b — arreglar el test `mutation_journals_reports_an_unrecognized_kind_as_unknown_format_without_panicking`

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Rol: worker de corrección (Rust). Orquestador: Claude Fable 5.1. Sin subagentes. **Nunca en segundo plano: la herramienta Bash acepta `timeout` hasta 600000 ms — úsalo para `cargo`; `run_in_background` está PROHIBIDO y termina tu sesión sin informe.** No commit. No Docker. **Archivos permitidos:** `crates/mcp-server/tests/doctor.rs`, `crates/mcp-server/src/doctor.rs` (solo si el defecto está en el producto), `crates/project-adapter/tests/support/native_mutation.rs` (solo si el fixture nuevo de W14 falla).

W14 (`docs/validation/M8/delegation/W14-doctor-journals/report.md`) no pudo compilar ni ejecutar en su sesión. El orquestador ejecutó: `cargo fmt` limpio, `cargo clippy -p rust-engineering-mcp -p rust-engineering-project --all-targets -D warnings` limpio, `cargo test -p rust-engineering-mcp --test doctor` → **4/5**, falla `mutation_journals_reports_an_unrecognized_kind_as_unknown_format_without_panicking` con `Error: "Rejected(InvalidProject)"`. Diagnostica (probablemente el test fabrica el journal de `operation_kind` desconocido de una forma que el store o `doctor` rechaza antes como proyecto/ruta inválida — p. ej. `--state-root` no canónico, tuple `--docker/--rust-image` incompleto, o bytes del envelope que no pasan la validación previa al kind). El objetivo de la decisión D12 §3 (`docs/validation/M8/03.md`): un journal cuyo `operation_kind`/formato este binario no interpreta debe contarse en `unknown_format` sin pánico y con `downgrade_blocked: true`. Arregla el test (o el producto si el comportamiento real contradice la decisión, explicándolo) y ejecuta:

```text
cargo test -p rust-engineering-mcp --locked --offline --test doctor
cargo fmt --all -- --check
cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings
```
Comprueba también con `git status --short crates/mcp-server/tests/snapshots` que solo `doctor-report.json` está modificado. Informe (Task / causa raíz / Files changed / salidas) en `docs/validation/M8/delegation/W14b-doctor-test-fix/report.md` y en tu última respuesta. No commit.
