# W06c — a dedicated references fixture so the m6-09 oracle actually discriminates a use from a declaration

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`). Role: implementation worker (test + fixture only). Orchestrator: Claude Fable 5.1 (running as Opus 4.8). You may not spawn subagents. **Never run a command in the background.** Do not run Docker-backed tests. Do not commit.

## Why (orchestrator finding, reproduced against the real M6 image)

The native cut `m6-09-references` failed: it queried `add` in `fixtures/valid-basic` and asserted "a declaration plus at least one call site", but `valid-basic`'s only use of `add` is inside a `#[test] fn`, which rust-analyzer cfg-excludes under the M6 minimal config (`noDeps`, no test cfg, no build scripts). So references returns **only the declaration** — the product is correct, the fixture is wrong. Cross-crate uses (`fixtures/workspace`) are also not found under `noDeps`. A **same-file, non-test** use is needed. Confirmed against the real binary: a crate with

```rust
pub fn add(a: u32, b: u32) -> u32 { a + b }
pub fn twice(x: u32) -> u32 { add(x, x) }
```

queried at `add` (line 1, column 8) returns, with `include_declaration: true`, exactly two references — the use in `twice` at line 2 columns 31–34 (`is_declaration: false`) and the declaration at line 1 columns 8–11 (`is_declaration: true`), `omitted_declarations: 0`; with `include_declaration: false`, exactly the use at line 2 (declaration removed), `omitted_declarations: 1`, `completeness: complete`.

## Deliverables

1. **New fixture `fixtures/analyzer-references/`**: `Cargo.toml` (package `analyzer-references`, `version = "0.1.0"`, `edition = "2024"`, no dependencies, not a workspace member — mirror `fixtures/valid-basic/Cargo.toml`'s shape exactly, including any `[lints]`/publish fields it carries) and `src/lib.rs` with exactly the two functions above (`add` on line 1, `twice` on line 2). Confirm it is a valid offline crate: `cargo check --manifest-path fixtures/analyzer-references/Cargo.toml --locked --offline` (or whatever flags `fixtures/valid-basic` builds with) succeeds. If `scripts/test-fixtures.py` (the `cargo-fixtures` gate) enumerates fixtures against an allowlist or expects each to build, make the new fixture satisfy it (read that script; if it just discovers and builds every fixture dir with a Cargo.toml, no change needed beyond a buildable crate — confirm).

2. **Repoint `m6-09-references`** in `crates/execution-adapter/src/analyzer_native.rs` to `fixtures/analyzer-references`, querying `add` at `Position::new(1, 8)`. Assert, against the real answer: `status`/answered, `completeness.state == Complete`, exactly **two** references with `include_declaration: true`; **exactly one** has `is_declaration == true` and it is the line-1 range (cols 8–11) that slices to `add` from that file's bytes; the other (`is_declaration == false`) is the line-2 use whose range (cols 31–34) also slices to `add`. Keep the existing file-aware slicing (index each reference against `reference.file`'s bytes). Record the reference count and declaration count in the cut receipt.

3. **Repoint the references end-to-end test** in `crates/mcp-server/tests/analyzer_runtime.rs` to the same fixture: `include_declaration: true` → 2 refs, exactly 1 declaration; then a second call `include_declaration: false` → 1 ref (the use), `omitted_declarations == 1`, and no reference is flagged declaration. Canonicalize the fixture root (the server refuses non-canonical `--root`).

4. Leave `fixtures/valid-basic` and every other cut untouched (the symbols cut and its e2e test still use `valid-basic`). Leave `m6-10-diagnostics` untouched.

## Verification (foreground)

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo check --manifest-path fixtures/analyzer-references/Cargo.toml --locked --offline
cargo test -p rust-engineering-execution --locked --offline --lib analyzer_native --no-run
cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run
python3 -B scripts/test-fixtures.py . --cargo "$(command -v cargo)"   # if it runs offline; if it needs Docker/network, skip and say so
python3 -B scripts/docs-hygiene.py links-check
```

Do not run the `#[ignore]` native cuts (the orchestrator runs the 11-cut suite). Report: Task / Result / Files changed / Tests executed / Evidence / Risks / Decisions / Open issues.
