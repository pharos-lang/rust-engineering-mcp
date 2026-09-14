# W12b — cerrar los dos P3 de V12 sobre el allowlist de argv del guest

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort high --disallowedTools Agent Task`) |
| CLI | `claude` 2.1.267 |
| Inicio / fin (UTC) | 2026-09-13T03:15:50Z / 2026-09-13T03:19:39Z |
| Resultado | `subtype: success`, `is_error: False` |
| P3-1 | `--target` cerrado a `GUEST_TARGET_TRIPLE="aarch64-unknown-linux-gnu"` (corrección de código, estrictamente más conservador) |
| P3-2 | límite de fronteras-de-argumento del oráculo documentado en el corte m6-03 y en la enmienda ADR-084 §7 (deuda trazada) |

---

## Report

**Task:** Close the two P3 findings from V12 on the guest argv allowlist in `crates/execution-adapter/src/analyzer_native.rs` — pin `--target` to the single M6 guest triple (code fix) and document the oracle's argument-boundary limitation (comment/ADR only).

**Result:** Both P3s closed. `--target` in `rustc_is_readonly_probe` now accepts only the exact constant `GUEST_TARGET_TRIPLE = "aarch64-unknown-linux-gnu"`, closing the disk-read path where a non-guest triple like `hostile` would have rustc search for `hostile.json` on `RUST_TARGET_PATH`/`/source`/sysroot. `--crate-name`/`--crate-type` are unchanged (still `plain_word`). The oracle's argument-boundary limitation (`docker container top`/`ps` joins argv with spaces; `admitted_argv` re-splits on whitespace) is now documented next to the existing sampled-instants note in the `m6_initialize_spawns_only_the_expected_guest_programs` cut comment, and reflected in one added sentence in the ADR-084 §7 2026-09-12 (W12) amendment.

**Files changed:**
- `crates/execution-adapter/src/analyzer_native.rs` — new `GUEST_TARGET_TRIPLE` const; `--target` arm split out of the `--crate-name`/`--crate-type` match arm and pinned to the constant; doc comments updated on `rustc_is_readonly_probe` and the constant; P3-2 note added to the `m6-03` cut comment; two new `refused` cases in the unit test.
- `docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md` — §7 W12 amendment text updated: `--target` described as pinned to the guest triple (not "a plain word"), plus one sentence on the sampling oracle's argument-boundary limitation.

**The exact `--target` arm now:**
```rust
"--target" => {
    if rest.next() != Some(GUEST_TARGET_TRIPLE) {
        return false;
    }
}
```
(`GUEST_TARGET_TRIPLE = "aarch64-unknown-linux-gnu"`)

**Tests:** Added to `refused` in `the_guest_argv_allowlist_admits_the_expected_forms_and_nothing_else`:
- `"/opt/rust/bin/rustc --target hostile --print cfg"`
- `"/opt/rust/bin/rustc - --crate-name ___ --target evil --print=cfg"`

All prior `admitted`/`refused` cases retained (including the real batched probe using `--target aarch64-unknown-linux-gnu`, and the pre-existing `--target /source/hostile.json` refusal). `cargo test -p rust-engineering-execution --lib analyzer_native`: 5 passed, 0 failed, 12 ignored (Docker-gated M6 tests, unaffected).

Full verification, all clean:
- `cargo fmt --all -- --check` — no diff
- `cargo check -p rust-engineering-execution --all-targets --locked --offline` — clean
- `cargo clippy -p rust-engineering-execution --all-targets --locked --offline -- -D warnings` — clean
- `cargo test -p rust-engineering-execution --lib --locked --offline analyzer_native` — 5 passed
- `python3 -B scripts/check-architecture.py` — PASS

**Risks:** None identified — the change is strictly more conservative (a subset of what was previously admitted for `--target`); no other code path reads that constant differently.

**Open issues:** None from this scope. P3-2 remains a documented, not fixed, limitation of the out-of-guest sampling oracle (per V12's own disposition — not cheaply fixable without `/proc/<pid>/cmdline`, tracked as debt M6-06 per the orchestrator's disposition note). No commit made, as instructed.
