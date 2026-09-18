# W39 — disposición del orquestador (2026-09-17)

Invocación: `claude -p --model sonnet --effort high --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md` (CLI 2.1.274). Inicio 2026-09-17T17:19:17Z, fin 17:41:26Z, exit 0, 85 turnos, 1 292 948 ms, modelos `claude-sonnet-5` (+ auxiliar `claude-haiku-4-5`), `permission_denials: 4` (dos `cargo test … | tee /tmp/…` con redirección a `/tmp`, dos `rustup target list`; sin efecto en el resultado). Relanzamiento tras la pérdida del worker original por reinicio del host; el prompt no cambió.

Veredicto: **aceptado**. Verificación del orquestador sobre el árbol resultante:
`cargo fmt --all -- --check` limpio; `cargo clippy -p rust-engineering-mcp
--all-targets -D warnings` limpio; `cargo test -p rust-engineering-mcp` → unit
479 passed / 3 ignored, `protocol` 61, `cli` 23, `doctor` 10, `capabilities_cli`
9, `catalog_cli` 5, `rmcp_tasks_spike` 5 y el resto en verde; snapshots sin
cambios. Cambios en producto: dos refactors puros (`doctor::classify_mutation_records`,
`mutation_cli::{build_report, render}`) sin cambio de comportamiento ni de
contrato (`contract-freeze.py verify --strict` sigue `passed`). Cobertura de
archivo (macOS, `--bins`): `doctor.rs` 59,96 → 75,44 %, `mutation_cli.rs` 37,36 →
87,11 %, `host_config.rs` 76,81 → 82,22 %. `contract_cli.rs`/`main.rs` no se
tocaron (fuera de la lista de archivos permitidos del encargo; contradicción del
propio prompt-header, resuelta a favor de la lista).

Hallazgo del worker adoptado por el orquestador: los helpers de spawn de los
tests de integración usan `env_clear()` y borran `LLVM_PROFILE_FILE`, por lo que
`protocol.rs`/`cli.rs` no aportan cobertura al binario en ningún host → encargo
[W39b](../W39b-llvm-profile-passthrough/prompt-header.md) (reinyección de esa
única variable). El resultado real de `new_coverage` se lee en SonarCloud tras
el push.
