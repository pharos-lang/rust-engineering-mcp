# W06d — informe del worker (Opción A)

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort high --disallowedTools Agent Task`) |
| Inicio / fin (UTC) | 2026-09-12T18:13:26Z / 2026-09-12T18:21:21Z; 474558 ms |
| Resultado | `subtype: success`, `is_error: False` |
| Transcript | fuera del árbol; hashes en `transcripts.sha256` |

Owner: **Opción A** (2026-09-12). `diagnostics.experimental.enable=false` sin
tocar (config_digest `a2592cfc…` intacto); el oráculo de no-build-script pasa a
símbolos (`GENERATED` ausente). Deuda de diagnósticos trazada.

## Informe literal del worker

This looks correct and matches the task's required assertions. Both cargo check/clippy passed and the tests compile. This completes the implementation.

## Report

**Task:** Fix the M6 build-script test oracle (native cut `m6-10` + e2e test) from a diagnostic-code assertion to the symbols-based no-expansion proof (Option A, 2026-09-12), and correct the affected docs/matrix/ADR honestly. No product-logic or config change.

**Result:** Done. `diagnostics.experimental.enable` was not touched (verified via `configuration_keys_flatten_to_the_dotted_paths_the_server_knows`, unchanged, still asserting 17 keys). All verification commands pass.

**Files changed:**
- `crates/execution-adapter/src/analyzer_native.rs` — renamed `m6_diagnostics_on_build_script_prove_no_build_script_ran` → `m6_no_build_script_runs_the_include_stays_unexpanded`; now makes two calls in one session: (a) `diagnostics` asserted answered/`Complete`/`health: ok` only (no code/emptiness assertion, count recorded in the receipt); (b) `symbols` (document scope) asserted `Complete`/`health: ok` and containing `generated_fact` but not `GENERATED` (both symbol lists recorded).
- `crates/mcp-server/tests/analyzer_runtime.rs` — same shape as an e2e test, renamed to `analyzer_diagnostics_and_symbols_on_build_script_prove_no_build_script_ran_on_the_real_m6_image`; root already canonicalized via `fixture_root_named`.
- `docs/tools.md` — new honest paragraph stating `rust.analyzer.diagnostics` is syntax-only under the minimal config (type/borrow/lint/unresolved-item errors are `rust.check`'s domain), points at the debt entry, and rewrote the build-script oracle paragraph around the symbols/no-expansion proof.
- `docs/security-model.md` — M6 build-script oracle row now cites the symbols/no-expansion proof; notes diagnostics quality as tracked debt.
- `docs/validation/M6/01.md` — R1 corrected: no in-band diagnostic exists with experimental diagnostics off; the deterministic proof is the unexpanded `include!` (absent `GENERATED`).
- `docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md` §3 — added "Enmienda 2026-09-12 (Opción A)" recording the calibration finding and the std-macro-noise reason Option B was rejected, `diagnostics.experimental.enable` unchanged (`false`).
- `docs/validation/M6/matrix.md` — M6-03 row updated to "Entregado; diagnósticos de sintaxis (experimental off, Opción A); prueba de no-build-script por símbolos; calidad de diagnósticos = deuda"; added a new "Deuda de M6" section (chose matrix.md over `adr-backlog-m2-m8.md`, which is a closed D01–D26 decision backlog, not a post-milestone debt tracker).

**Tests executed:**
- `cargo fmt --all -- --check` — clean
- `cargo check --workspace --all-targets --locked --offline` — clean
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` — clean
- `cargo test -p rust-engineering-execution --locked --offline --lib analyzer_native --no-run` — compiles
- `cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run` — compiles
- `python3 -B scripts/docs-hygiene.py links-check` — 0 broken in living documents (unchanged 459 pre-existing broken links in frozen records)

**Evidence:** config_digest-affecting unit test (`configuration_keys_flatten_to_the_dotted_paths_the_server_knows`) untouched and still asserts 17 fixed keys including `diagnostics.experimental.enable`.

**Risks:** None identified — no config/product-logic changed; both `#[ignore]`d native/e2e tests were not run here (per instructions, the orchestrator runs the 11-cut suite and the real-image e2e).

**Decisions:** Placed the M6-03 debt entry as a new "Deuda de M6" subsection in `matrix.md` rather than `adr-backlog-m2-m8.md`, since that file is a closed decision backlog (D01–D26), not a debt tracker, matching the task's fallback instruction.

**Open issues:** None — did not need any assertion the real binary's behavior contradicts; if the orchestrator's calibration run against the real M6 image finds `generated_fact`/`GENERATED` symbol names differ from what's asserted, that would need to be reported back, but nothing in this session touched or ran Docker to verify against the real image.
