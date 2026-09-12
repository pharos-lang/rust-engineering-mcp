# W05e — M6-01 native tool evidence doc, and de-gate + harden the flaky `analyzer_runtime` wrapper

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Role: implementation worker. Orchestrator: Claude Fable 5.1 (running as Opus 4.8 this turn). You may not spawn subagents. **Never run a command in the background** (the `-p` session ends at the end of your turn and is not resumed); run everything in the foreground and end with the report as your last message. Do not run Docker-backed tests (no Docker for you). Do not commit.

## Context (orchestrator decision, owner-approved Option A, 2026-09-12)

The nine `analyzer_native.rs` calibration cuts pass 9/9 against the real M6 image and are the **calibrated native evidence** for M6-01 (receipt `docs/validation/M6/01-calibration.json`). W05 additionally added an end-to-end wire test `crates/mcp-server/tests/analyzer_runtime.rs` and wired it into `scripts/test-m6-runtime.py` as a tenth gate selection. That wrapper is **flaky at the harness level**: it errors `Error: Timeout` (~65 s) under cargo's test harness, while the product path it exercises passes on every manual reproduction (see "Recorded manual evidence" below). The orchestrator reproduced the exact call sequence with the test's exact per-call budgets and canonical state/root and it passed every time; the standalone `cargo test --test analyzer_runtime -- --ignored` still failed with `Timeout`, root cause not isolated. Decision: the e2e wrapper must **not** be a blocking gate selection until it is self-diagnosing and reproducible; the 9 calibrated cuts remain the gate.

## Recorded manual evidence (verbatim from the orchestrator's runs; use these exact values in the doc)

Command shape (host macOS ARM64):
`target/debug/rust-engineering-mcp serve --stdio --root <canonical fixtures/valid-basic> --docker /Applications/Docker.app/Contents/Resources/bin/docker --docker-socket /Users/cburgosro/.docker/run/docker.sock --state-root <canonical temp dir> --rust-image sha256:f39a5b33ee7d54243664162eb635f8ec223d512042beb7cd18ecf071046b310c`, handshake either the 2026-07-28 `_meta` convention or a classic `initialize`+`notifications/initialized` (both verified).

- `tools/list` → 32 tools; last two are `rust.binary.bloat`, `rust.analyzer.symbols`.
- `rust.project.open` (path = the same canonical root) → `passed`; e.g. `project_ref prj_5c5581874e113161aba64919836c5f1d`, `fingerprint sha256:0e3de52d49e08456704ef13eb1877d64b9caa2022618da9a4fc3c1f44b1d742b`.
- `rust.analyzer.symbols {scope:document, file:"src/lib.rs"}` → `status passed`; `analyzer.version "rust-analyzer 1.98.1 (48a229c 2026-09-01)"`, `binary_sha256 sha256:a0c3f11a153e5d5f12a6de6ceabd2def60c2793293e901a4a045946050456e2f`, `image_id sha256:f39a5b33…`, `config_digest sha256:a2592cfc4e65af0b5ff6a36ff737bbc32ef7d5073af65b2a7ee36907d49981b4`, `position_encoding "utf-8"`; `readiness {state quiescent, health ok, elapsed_ms 323}`; `completeness {state complete, omissions [], reasons []}`; `session {messages_in 5, messages_out 6, bytes_in 3804, bytes_out 1934, duration_ms 353, server_requests 0, stderr_bytes 0}`; symbols `add` (function, line 1 cols 8–11) and `addition` (function, line 3); latency ~14.4 s cold (first call, includes gateway calibration), ~1.0–1.1 s warm on repeats.
- `rust.analyzer.symbols {scope:workspace, query:"add"}` → `status passed`, returns `add`/`addition`, `completeness complete`, ~2.2 s.
- Server exits 0; no leftover `org.rust-mcp.execution=true` container or volume.
- Environmental trap (already partly fixed in W05d): a **non-canonical** `--state-root` or `--root` (e.g. `/var/folders/.../T/…`, a symlink to `/private/var/folders/…`) makes gateway calibration fail `SANDBOX_DENIED` in ~0.2 s; the canonical form passes in ~14.4 s. Both paths must be canonicalized.

## Deliverables

### D1 — `docs/validation/M6/02.md` (new): M6-01 tool evidence

Write it in the register style of `docs/validation/M6/01.md`. Sections: what M6-01 delivers (`rust.analyzer.symbols`, tool 32); the **gate evidence** = the nine calibrated `analyzer_native` cuts (link `01-calibration.json`, receipt sha256, 9/9); the **product end-to-end evidence** = the recorded manual reproduction above, presented as measured facts (exact command, handshake, the four calls with their statuses/timings/identities/session counts), explicitly labelled as an orchestrator manual reproduction, not an automated receipt; the **known issue** = the `analyzer_runtime` cargo wrapper is flaky (`Timeout` ~65 s, not reproduced standalone, root cause not isolated), de-gated pending W05f hardening, tracked here; the environmental canonicalization trap; residual risks. Do not claim the wrapper passes. Do not overstate: "manual reproduction" is auxiliary product evidence, the calibrated cuts are the gate.

### D2 — De-gate the wrapper in `scripts/test-m6-runtime.py`

Make the gate driver discover and run **only** the nine `analyzer_native.rs` `#[ignore]` cuts (its original M5-style design), not `crates/mcp-server/tests/analyzer_runtime.rs`. If W05 added an integration-test discovery step, revert exactly that; keep everything else (receipt shape, cleanup, digest cross-checks). Update the unit tests `scripts/test-m6-runtime-unit.py` accordingly. Update any sentence in `docs/ci.md` that states the number of `m6-runtime` selections (it should say nine calibrated cuts; the full-gate stage count 41/42 is unchanged because `m6-runtime` is one stage). If the count was never stated numerically, leave it.

### D3 — Harden `crates/mcp-server/tests/analyzer_runtime.rs` so a future failure is self-diagnosing

Keep it an `#[ignore]` end-to-end test (still runnable manually and by a future gate once trusted), but:
- On any timeout, `Server::response` must report **which request id** timed out, how long it waited, whether the child is still alive (`try_wait`), and drain the bounded stderr (like `exit_error` already does) — never a bare `Timeout`/`Disconnected`.
- Read the reader-thread + `sync_channel` path for a plausible hang: e.g. does the stdout reader thread block on `tx.send` if the test stops reading, or can a large single frame stall `read_until`? If you find a concrete defect that explains a ~65 s stall with no matching budget, fix it and say exactly what it was; if you cannot, say so plainly and leave the diagnostics improvement as the deliverable (do not invent a root cause).
- Canonicalize both `--root` and `--state-root` (verify W05d did the root; do the state-root too).
- Compile it: `cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run`. Do not run the ignored test.

## Verification (targeted, foreground)

```text
cargo fmt --all -- --check
cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run
python3 -B scripts/test-m6-runtime-unit.py
python3 -B scripts/test-gate-reporting.py
python3 -B scripts/docs-hygiene.py links-check
```

## Ownership

`docs/validation/M6/02.md` (new); `scripts/test-m6-runtime.py`; `scripts/test-m6-runtime-unit.py`; `crates/mcp-server/tests/analyzer_runtime.rs`; `docs/ci.md` (only an m6-runtime selection-count sentence, if present). Nothing else. No `unwrap`/`expect`/`panic!`/`unsafe` in the Rust test beyond what the harness already uses (it returns `Result`, keep that).

## Report (mandatory headings)

Task / Result / Files changed / Tests executed / Evidence / Risks / Decisions / Open issues.
