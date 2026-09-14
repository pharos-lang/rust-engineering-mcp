# W05 — disposición del orquestador (2026-09-14)

Veredicto: **aceptado, pendiente de revisión V02 (Opus High)**. Verificación
directa del orquestador sobre los bytes entregados:

| Comprobación | Resultado |
| --- | --- |
| Snapshots | Solo los 5 `analyzer-*-tool.json` cambian (una línea cada uno: prefijo `Preview (ADR-086): ` en `description`); los 31 restantes byte-idénticos (`git status`) |
| `cargo run -q -p rust-engineering-mcp --locked --offline -- contract --json` | Exit 0, 17 678 bytes; `document_kind: rust_engineering_capabilities`, `format_version 1`, protocolo primario `2026-07-28` + 4 legacy, `sdk rmcp 3.2.0`, `tool_count 36`, 31 `stable` / 5 `preview`, 2 Resources dinámicas |
| Hashes canónicos | Recalculados en Python (`json.dumps(sort_keys, separators, ensure_ascii=False)`) para las 36 tools: **0 discrepancias** en `input_schema_sha256`, `output_schema_sha256`, `description_sha256`; annotations idénticas a los snapshots |
| Manifiesto | `contract-freeze.py generate` → `freeze-0.8.0.json` (31/5); `verify --strict` → `passed`; hashes del manifiesto == hashes de `contract --json` (0 discrepancias) |
| `cargo fmt --all -- --check` | Limpio |
| Tests | Salida del `cargo test -p rust-engineering-mcp` del worker (hash en `transcripts.sha256`): todas las suites `ok`, 0 fallos |
| Diseño | Cada tool expone un `definition()` estático (contrato + `Tool`) reutilizado por el constructor y por `capability_document.rs`; `stability.rs` tabla cerrada; `contract_cli.rs` subcomando estático sin Docker/red; `domain`/`application` sin cambios |

Pendiente: clippy `-D warnings` y `check-architecture.py` se cubren en el gate
`core` de la integración; revisión de contrato/CLI por V02.
