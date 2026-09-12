# W05e — informe del worker

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort high --disallowedTools Agent Task`), Claude Code 2.1.268 |
| Inicio / fin (UTC) | 2026-09-12T14:04:28Z / 2026-09-12T14:11:59Z; 449688 ms |
| Resultado | `subtype: success`, `is_error: False`, permisos denegados 3 |
| Transcript | fuera del árbol; hashes en `transcripts.sha256` |

Decisión Opción A del owner (2026-09-12): la evidencia de gate de M6-01 son los
nueve cortes calibrados de `analyzer_native.rs`; el wrapper e2e
`analyzer_runtime.rs` se **desgatea** (flake de arnés `Timeout` ~65 s no
reproducido a solas) y se endurece para que un fallo futuro diga qué petición
expiró. El producto se reprodujo de extremo a extremo a mano (recibo en
[02.md](../../02.md)). W05e halló además una carrera real en `finish()` del
arnés (el hilo lector podía bloquearse al entregar frames finales a un canal
lleno), corregida; no confirma que sea *la* causa del Timeout y lo dice.

## Informe literal del worker

This looks correct. Everything is in order — let's produce the final report.

## Task

W05e: write M6-01 tool evidence doc (`docs/validation/M6/02.md`), de-gate the flaky `analyzer_runtime` e2e wrapper from the M6 runtime gate driver, and harden that wrapper's diagnostics so a future failure is self-diagnosing.

## Result

All three deliverables are done. D2's revert restores `scripts/test-m6-runtime.py` to running exactly the nine `analyzer_native.rs` cuts. D3 replaced the bare `Timeout`/`Disconnected` errors in `analyzer_runtime.rs`'s `Server::response` with diagnostics naming the request id, elapsed wait, child liveness, and drained stderr, and fixed a genuine (if unconfirmed as *the* root cause) race in `finish()` where the reader thread could block handing off trailing frames to a full, undrained channel. D1's `docs/validation/M6/02.md` was already expected by `matrix.md`'s existing link — creating it fixed a broken-link failure that `docs-hygiene.py links-check` was already flagging.

## Files changed

- `docs/validation/M6/02.md` (new) — M6-01 tool evidence: gate evidence (9/9 calibrated cuts), manually-reproduced product evidence, the known wrapper flake, the canonicalization trap, residual risks.
- `scripts/test-m6-runtime.py` — removed the `INTEGRATION_SOURCE`/`INTEGRATION_PACKAGE`/`INTEGRATION_TEST_BINARY` constants, the integration-test discovery/validation block, the `integration_selections` receipt key, and the tenth-step execution loop. Kept the `run_step` extraction and the `RUST_MCP_TEST_DOCKER` allowlist fix (both are generic and needed by the native cuts themselves, not integration-specific).
- `crates/mcp-server/tests/analyzer_runtime.rs` — `Server::response` now calls a new `stall_error` on any timeout path (explicit zero-remaining check, `RecvTimeoutError::Timeout`, and `RecvTimeoutError::Disconnected`), reporting id/elapsed/`try_wait`/stderr. `finish()` now drains `self.stdout` non-blockingly each poll iteration.

## Tests executed

- `cargo fmt --all -- --check` — clean.
- `cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings` — clean.
- `cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run` — compiles; ignored test not run (no Docker here).
- `python3 -B scripts/test-m6-runtime-unit.py` — 19/19 pass (no changes needed to this file; it never referenced the integration wiring).
- `python3 -B scripts/test-gate-reporting.py` — 13/13 pass.
- `python3 -B scripts/docs-hygiene.py links-check` — went from 1 broken link in living documents (`matrix.md:34 -> 02.md`) to 0.

## Evidence

- `docs/ci.md` has no sentence stating the `m6-runtime` selection count numerically (only the 25/26-core, 41/42-full stage-count sentence, which is unaffected since `m6-runtime` is still one stage) — left untouched per the instruction "if the count was never stated numerically, leave it."
- `docs/validation/M6/matrix.md` already carried a pre-existing link to `02.md`, confirming the doc was expected before this delegation started.

## Risks

- The `analyzer_runtime` wrapper's ~65s `Timeout` remains unexplained; the `finish()` drain fix addresses a real, demonstrable race (matching the documented pattern in the sibling harness `inspection_runtime.rs::finish_expect`) but I could not reproduce the failure to confirm it as *the* cause, and said so explicitly in `02.md` rather than overclaiming.
- None of this was validated against Docker/the real M6 image in this session (no Docker available to this worker) — only compiled and statically reviewed.

## Decisions

- Kept the `run_step()` extraction and `RUST_MCP_TEST_DOCKER` allowlist addition in `test-m6-runtime.py` rather than reverting the whole file to its pre-W05 state, since both are generic fixes needed by the native cuts themselves (the latter is read by `analyzer_native.rs` too) and not specific to the integration-test wiring being removed.
- Verified rather than modified the `--root`/`--state-root` canonicalization in `analyzer_runtime.rs`: both were already canonicalized (matching the established pattern in `inspection_runtime.rs`/`cli.rs`), so no code change was needed there — only confirmed and noted in `02.md`.

## Open issues

- W05f (not this package) must isolate and confirm the actual root cause of the `analyzer_runtime` ~65s `Timeout` and, once trusted, re-gate it as the tenth `m6-runtime` selection.
