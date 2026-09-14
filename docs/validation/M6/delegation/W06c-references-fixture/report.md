# W06c — informe del worker

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort medium --disallowedTools Agent Task`) |
| Inicio / fin (UTC) | 2026-09-12T17:30:37Z / 2026-09-12T17:40:56Z; 618200 ms |
| Resultado | `subtype: success`, `is_error: False` |
| Transcript | fuera del árbol; hashes en `transcripts.sha256` |

Contexto: el corte `m6-09-references` de W06 esperaba «declaración + un uso»,
pero el único uso de `add` en `valid-basic` está en un `#[test] fn` que
rust-analyzer excluye por cfg con la config mínima M6 (el producto es correcto;
el orquestador lo reprodujo contra el binario real). W06c añade
`fixtures/analyzer-references/` (dos funciones en el mismo archivo, un uso
no-test) y repunta el corte y el test e2e a las aserciones exactas medidas.
El orquestador retiró el `target/` de build que el worker dejó (no se versiona)
y ejecuta la calibración de 11 cortes.

`scripts/test-fixtures.py` no enumera el filesystem: llama `compiler_case` por
nombre desde una allowlist `BENIGN` fija, así que una fixture nueva no listada
no rompe la gate ni exige entrada en `corpus-sha256.json`.

## Informe literal del worker

fmt check clean, no unintended drift. Everything is in place, uncommitted per instructions.

## Report

**Task**: Fix `m6-09-references` cut to use a fixture where an actual (non-test) use of a symbol exists, so declaration-vs-use discrimination is real, not vacuous.

**Result**: Done. New fixture `fixtures/analyzer-references/` added; both the native cut and the MCP e2e test repointed to it and tightened to exact assertions.

**Files changed**:
- `fixtures/analyzer-references/Cargo.toml`, `fixtures/analyzer-references/src/lib.rs`, `fixtures/analyzer-references/Cargo.lock` (new) — `add` on line 1, `twice` (calling `add`) on line 2, mirroring `valid-basic`'s manifest shape.
- `crates/execution-adapter/src/analyzer_native.rs` — `m6-09-references` cut renamed to `m6_references_on_analyzer_references_flag_the_declaration_exactly_once`, now asserts exactly 2 references, exactly 1 declaration, declaration at line 1 col 8 and the use at line 2 col 31, both slicing to `"add"`; cut receipt records `reference_count`/`declaration_count`.
- `crates/mcp-server/tests/analyzer_runtime.rs` — e2e test repointed to `fixtures/analyzer-references` (canonicalized root via existing `fixture_root_named`); `include_declaration: true` asserts exactly 2 refs / 1 declaration / `omitted_declarations: 0`; added a second call with `include_declaration: false` asserting exactly 1 ref, none flagged declaration, `omitted_declarations: 1`.

**Tests executed** (all foreground, offline):
- `cargo fmt --all -- --check` — clean
- `cargo check --workspace --all-targets --locked --offline` — passes
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` — clean
- `cargo check --manifest-path fixtures/analyzer-references/Cargo.toml --locked --offline` — passes
- `cargo test -p rust-engineering-execution --locked --offline --lib analyzer_native --no-run` — compiles
- `cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run` — compiles
- `python3 -B scripts/docs-hygiene.py links-check` — 0 broken in living documents (unchanged baseline: 459 broken in frozen records, pre-existing)
- `scripts/test-fixtures.py` — attempted; failed locally only because it requires a pinned real Cargo 1.98.1 binary (not a rustup shim), which this machine doesn't have. Confirmed by reading the script that it enumerates a fixed `BENIGN` allowlist and `corpus-sha256.json` pin set that does **not** include `analyzer-references`, so this fixture needs no changes there and the script's behavior is unaffected by this work.

**Evidence**: New fixture's `add`/`twice` positions verified programmatically (`add` at line 2 col 31–34) before wiring into both tests, matching the orchestrator's real-binary reproduction in the prompt exactly.

**Risks**: None — `fixtures/valid-basic` and `m6-10-diagnostics` untouched as instructed. Did not run the `#[ignore]` native/e2e cuts (no Docker here); orchestrator runs the 11/12-cut suite.

**Open issues**: None from this task. `test-fixtures.py`'s toolchain-pin requirement is a pre-existing local-environment limitation, unrelated to this change.
