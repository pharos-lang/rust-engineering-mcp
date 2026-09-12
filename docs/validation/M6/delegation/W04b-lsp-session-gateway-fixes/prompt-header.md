# W04b — Apply the V04 review dispositions and the F1/F2 decisions to the W04 package, then re-run the native calibration

Model requested: Claude Opus 5 (`claude -p --model opus --effort high`). Role: implementation worker, same ownership as W04 plus the three files named in item 1. Orchestrator: Claude Fable 5.1. You may not spawn subagents. Read `docs/validation/M6/delegation/V04-review-lsp-session-gateway/disposition.md` (the binding decisions), the review it disposes, and `docs/validation/M6/01.md` (your own findings F1/F2) before touching anything. Do not commit.

## Changes (all mandatory; numbering follows the disposition)

1. **F1 + F2 — fixed configuration.** In `crates/execution-adapter/src/lsp_codec.rs::initialization_options()` remove `cargo.sysrootQueryMetadata` and `cargo.autoreload` (18 → **17 keys**; the default `autoreload=true` applies). Update the doc comment. Amend `docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md` §3 and `docs/validation/M6/delegation/D25-D26-decision-brief.md` §4.5 with a dated one-paragraph amendment each ("Enmienda 2026-09-12 (calibración W04)") that removes the two keys and states why (key absent from the real schema; permanent `warning` from autoreload). The `config_digest` changes; record the new value in `01.md`.
2. **F2 — reason vocabulary.** Rename `domain::IncompleteReason::SysrootWarning` → `AnalyzerWarning` (any `health: warning`, message never published). Keep warning ⇒ `incomplete`. After the config change the calibration must observe `health: ok` at quiescent on `fixtures/valid-basic`; assert it in the document-symbols cut. If it is still `warning`, stop that cut, keep it failing, and report the exact `message` text in your report only (not in any receipt or doc).
3. **P2 — m6-03 oracle by full argv.** Replace the basename check with a closed argv allowlist: exactly `/opt/analyzer/bin/rust-analyzer` with no arguments; `/opt/rust/bin/rustc` only with `-vV`, `--print sysroot`, `--print cfg -O`, `-Z unstable-options --print target-spec-json`; `/opt/rust/bin/cargo` only with `--version`, `locate-project …`, `metadata --no-deps --format-version 1 …`, `rustc -Z unstable-options --print …`. Any other argv — in particular `rust-analyzer proc-macro …`, `build-script-build`, `rustfmt`, `sh` — fails the cut. Record every distinct argv observed in the cut document. Keep `MIN_PROGRAM_SAMPLES`.
4. **P2 — stale cut documents.** `scripts/test-m6-runtime.py` removes `target/m6-calibration/cut-*.json` and `receipt.json` before running; every cut publishes a `fail` document if it ends without reaching `pass` (a guard type whose `Drop` publishes when not marked passed); each cut document carries `run_started_at` and the driver requires `cut_status` keys == the selections it ran and every document's `run_started_at` ≥ the driver's start.
5. **P2 — non-UTF-8 files.** Add `domain::IncompleteReason::NotUtf8File`; push it whenever the omission is recorded; gateway unit test with a bundle containing an invalid-UTF-8 `.rs` file → `incomplete[not_utf8_file]` with the omission, never `Complete`, never `Infrastructure`.
6. **P2 — admission scope.** In `docs/adr/ADR-085-m6-runtime-admission.md` add a paragraph after the digest line: the M6 digest joins the global admission list like ADR-077 did for M5; tools M1–M5 remain qualified against their own digests and running them on the M6 image is not qualified by their suites; the M5→M6 difference is additive (`/opt/analyzer`, `rust-src` under `/opt/rust`); `docs/compatibility.md` declares it in M6-06. No code change.
7. **P3 — durations.** Keep `session.duration_ms` as measured by the session; add `call.duration_ms` for the whole call.
8. **P3 — `send` bounded by phase.** `send` takes the phase `until`; exceeding it classifies as `TimeoutInitialize`/`TimeoutQuery` by stage, never `NotReady`.
9. **P3 — kill/reap evidence.** Record `kill_error`/`reap_error` (as bounded strings of the io error kind, no paths) in `SessionOutcome`; if reaping does not confirm termination, `stop = KillUncertain`. Container-level absence remains the guarantee.
10. **P3 — deterministic EOF classification + failing unit test.** If stdout reached EOF, `stop = Eof` regardless of the subsequent kill; fix `a_request_cut_short_by_end_of_stdout_is_an_eof_failure` (currently fails at `lsp_session.rs:1137`) by making the classification deterministic, not by loosening the assertion. m6-07 asserts `evidence.killed`, `stop ∈ {Eof, Killed}` and the container `State.ExitCode == 137` read by `container inspect` before cleanup; drop the racy `exit_code.is_some()` assertion.
11. **P3 — frame-limit evidence.** Split `AnalyzerFailure::FramingRejected` into `FrameTooLarge` and `MalformedHeader` (map from the codec's `FrameLimit` vs `MalformedHeader`); m6-08 asserts `FrameTooLarge` and records the observed `Content-Length` in its document.
12. **P3 — `analyzer_configuration`** also scans `source.directories()`.
13. **P3 — cut notes.** Fix the m6-04 and m6-03 notes as the disposition says; the build-script diagnostics oracle is assigned to W07, say so.
14. **P3 — Python.** `OUTPUT` becomes the constant `ROOT / "target/m6-runtime-gate"` (no env override); `STEP_TIMEOUT_S` parsed with an explicit refusal (`RuntimeError` naming the variable) on non-integer/non-positive values; `owned_docker_state` uses `RUST_MCP_TEST_DOCKER` when set, else the known path. Add/adjust unit tests in `scripts/test-m6-runtime-unit.py` if you created it (else create it for these three behaviours and add it to the SonarCloud coverage list).
15. **Re-run everything.** `python3 -B scripts/test-m6-runtime.py` (with `RUST_MCP_TEST_SOCKET` and `RUST_MCP_TEST_IMAGE=sha256:f39a5b33ee7d54243664162eb635f8ec223d512042beb7cd18ecf071046b310c`) must pass all nine selections; copy the receipt and schema to `docs/validation/M6/01-calibration.json` / `01-config-schema.json` and rewrite `docs/validation/M6/01.md` so it describes this run (state, config digest, `health` observed, argv observed, timings, F1/F2 marked resolved, residual risks R1–R5 updated).

## Verification (targeted)

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-domain --locked --offline analyzer
cargo test -p rust-engineering-execution --locked --offline --lib lsp_codec
cargo test -p rust-engineering-execution --locked --offline --lib lsp_session
cargo test -p rust-engineering-execution --locked --offline --lib analyzer_gateway
cargo test -p rust-engineering-project --locked --offline --test source
python3 -B scripts/check-architecture.py
python3 -B scripts/test-gate-reporting.py
python3 -B scripts/test-m6-runtime-unit.py          # if present
RUST_MCP_TEST_SOCKET=… RUST_MCP_TEST_IMAGE=sha256:f39a5b33… python3 -B scripts/test-m6-runtime.py
python3 -B scripts/docs-hygiene.py links-check
```

Do not run the workspace-wide test suite or the gates. If the Claude Code CLI warns about background tasks at exit, make sure no cargo/docker process of yours is left running before you finish, and write your final report as the **last** message (Task / Result / Files changed / Tests executed / Evidence (receipt sha256, config digest, health, argv list, timings) / Risks / Decisions / Open issues).
