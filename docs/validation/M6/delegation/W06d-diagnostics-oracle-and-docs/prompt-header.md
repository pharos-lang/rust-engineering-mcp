# W06d — Option A: fix the build-script oracle to the symbols-based proof, label diagnostics honestly, and record the debt

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Role: implementation worker (test oracle + docs; **no config/product-logic change** — `diagnostics.experimental.enable` stays `false`). Orchestrator: Claude Fable 5.1 (running as Opus 4.8). You may not spawn subagents. **Never run a command in the background.** Do not run Docker-backed tests. Do not commit.

## Owner decision (Option A, 2026-09-12), backed by orchestrator measurements against the real M6 image

Under the M6 minimal config (`checkOnSave=false`, `procMacro.enable=false`, `diagnostics.experimental.enable=false`), `rust.analyzer.diagnostics` returns `[]` for real borrow/type/unresolved errors — it surfaces only the analyzer's syntax-level diagnostics. Enabling experimental diagnostics was rejected for now (it floods with false `unresolved-macro-call` for std macros `vec!`/`assert_eq!`/`#[test]` under the minimal config; tracked as debt). So:

- Keep `diagnostics.experimental.enable=false` (do not touch `lsp_codec::initialization_options()`; the config_digest `sha256:a2592cfc…` must not change).
- The "no build script ran" containment proof moves from **diagnostics** to **symbols**: measured against the real binary, `rust.analyzer.symbols` (document scope) on `fixtures/build-script/src/lib.rs` returns exactly `["generated_fact"]` and **not** `GENERATED` — i.e. `include!(concat!(env!("OUT_DIR"), "/generated.rs"))` did not expand (had a build script run, `generated.rs` would define `GENERATED` as a top-level item and it would appear). `rust.analyzer.diagnostics` on the same file returns `completeness: complete`, answered, no crash, empty list.

## Changes

1. **Native cut `m6-10`** in `crates/execution-adapter/src/analyzer_native.rs`: rename to something like `m6_no_build_script_runs_the_include_stays_unexpanded`. In one session (or two calls as the harness allows), assert: (a) `rust.analyzer.diagnostics` on `fixtures/build-script/src/lib.rs` is answered, `completeness.state == Complete`, `health: ok`, quiescent, session exits cleanly, cleanup verified (keep the existing lifecycle assertions); (b) `rust.analyzer.symbols` (document scope) on the same file contains a symbol named `generated_fact` and contains **no** symbol named `GENERATED` — the deterministic proof the build script did not run. Record in the cut receipt the symbol names observed and the diagnostics count. Do not assert any specific diagnostic `code` (there is none). If the real answer differs from the above, stop and report — do not weaken.

2. **End-to-end test** in `crates/mcp-server/tests/analyzer_runtime.rs` (the diagnostics-on-build-script test): same shape — diagnostics answered/complete on the build-script fixture, plus a symbols call asserting `generated_fact` present and `GENERATED` absent. Canonicalize the root.

3. **Docs — honest labelling.**
   - `docs/tools.md` (`rust.analyzer.diagnostics` section): state plainly that under the safe minimal config the tool surfaces the analyzer's **syntax-level** diagnostics; type, borrow, lint and unresolved-item errors that require `cargo check` are the domain of `rust.check`, not this tool; the containment proof that no build script ran is that the analyzer does not expand `include!(concat!(env!("OUT_DIR"),…))` (its generated symbols are absent), complementing the process-tree observation (`m6-03`). Add one sentence pointing at the tracked debt (item 5).
   - `docs/security-model.md`: fix the M6 build-script oracle row to the symbols/no-expansion proof (not a diagnostic).
   - `docs/validation/M6/01.md` R1: correct the "in-band diagnostics oracle" note — with experimental diagnostics disabled the analyzer emits no unresolved-macro diagnostic; the deterministic in-band proof is the unexpanded `include!` (absent generated symbol).
   - `docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md` §3: keep `diagnostics.experimental.enable=false`; add a dated "Enmienda 2026-09-12 (Opción A)" recording the calibration finding (the tool is syntax-only under the minimal config; enabling experimental floods with false std-macro-unresolved noise) and that a future decision may enable it once std-macro resolution is clean.

4. **Matrix** `docs/validation/M6/matrix.md`: M6-03 row → "Entregado; diagnósticos de sintaxis (experimental off, Opción A); prueba de no-build-script por símbolos; calidad de diagnósticos = deuda". Add a "Deuda trazada" note.

5. **Debt entry** in `docs/roadmap/adr-backlog-m2-m8.md` (or wherever post-milestone debt is tracked — check; if there's no such section, add a short subsection to `docs/validation/M6/matrix.md` titled "Deuda de M6"): "M6-03 diagnostics quality — enable rust-analyzer native semantic diagnostics (Option B) once std-library macro resolution under the minimal config is clean; today experimental diagnostics produce false `unresolved-macro-call` for `vec!`/`assert_eq!`/`#[test]`. Owner decision required before enabling."

## Verification (foreground)

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-execution --locked --offline --lib analyzer_native --no-run
cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run
python3 -B scripts/docs-hygiene.py links-check
```

Do not run the `#[ignore]` native cuts (the orchestrator runs the 11-cut suite). No product-logic/config change; if you find you need one to make an assertion pass, stop and report. Report: Task / Result / Files changed / Tests executed / Evidence / Risks / Decisions / Open issues.
