# M1-06 external R2 review

Claude Code2.1.259; explicit claude-opus-5 High; actual modelUsage verified.
Safe/restricted/tools disabled/read-only. Haiku bookkeeping was not the reviewer.
Packet90921bytes; before final runtime results.

## Independent R2 review — M1-06 (read-only, no tools used)

Scope reviewed: `ADR-037`, `domain/src/test_run.rs`, `application/src/test_run.rs`, `mcp-server/src/stdio/testing.rs` (+`schemas.rs`), `rust_gateway/test_runtime.rs`, and the integration diff (`parse_test`, `cargo_run`, `Phase::arguments`, stdio wiring, two runtime tests).

Overall: the argv is genuinely closed, the filter/timeout grammars match between the schema mirror and the domain validator, `Passed` is correctly conjunctive (`validation_complete && Exited && exit==0 && build_succeeded==Some(true)`), timeout is handled before the outcome match so it can never pass, and the `test_output` widening of `validation_complete` is gated so Check/Clippy semantics are untouched. No false-`passed` path found: every accepting branch still requires a genuine `exit_code == 0`.

### P0
None found.

---

### P1

**1. `parse_test` trusts the *first* well-formed `build-finished`, so a forged record truncates diagnostic parsing while still asserting `validation_complete: true`.**
`crates/execution-adapter/src/cargo_diagnostics.rs` (new `parse_test`, +421..+446): the loop returns on the first line that deserializes as `Event::Finished`, and only the `success == false` case is cross-checked against a non-empty tail:

```rust
if let Ok(Event::Finished { success }) = serde_json::from_str::<Event>(line) {
    let mut parsed = parse(&stdout[..offset], source, stream_complete)?;
    if !success && !stdout[offset..].is_empty() { parsed.complete = false; }
    return Ok(parsed);
}
```

Consequence if a `{"reason":"build-finished","success":true}` line reaches cargo's stdout *before* the real one: `parse` sees only the truncated prefix, so every subsequent real `compiler-message` record is dropped **without incrementing `diagnostics_omitted`**, `build_finished` is reported as `Some(true)`, and `project_inspection.rs` (+299..+309) still yields `validation_complete = true` for `(Some(1..), Some(true))`. The client then gets `failed` + "Cargo test failed after its reported build phase" + `validation_complete: true` + zero/partial diagnostics for a build that actually failed. This is an evidence-integrity regression relative to `parse`, which is strict: the diff's own test asserts `parse(prefix + "running 0 tests\n")` is **not** complete, i.e. under Check/Clippy any post-`build-finished` content forces incompleteness.

Reachability is the open question and is **not covered by any fixture in this change**: cargo forwards rustc's stdout to its own stdout, so compile-time code (proc macro `println!`) is the candidate producer; build-script stdout is captured by cargo and is not a vector. Under the stated threat model ("Output untrusted/project writable", compile-time arbitrary code already in scope for R1/R2), this should not be left unverified.

Recommendation, cheapest sound fix: after accepting a `success == true` record, scan the tail for any line that deserializes as `Event`; real Cargo never emits Cargo JSON after `build-finished`, so its presence means either forgery or a harness impersonating Cargo — set `complete = false` in both cases (fails safe, never affects status *up*). This contradicts the current test at `cargo_diagnostics.rs` tail case `"not JSON\n{\"reason\":\"build-finished\",\"success\":false}\n"` which asserts `parsed.complete`; that assertion encodes the "tail is fully opaque" decision and is exactly the decision I'm flagging. If you keep opacity instead, add a runtime fixture (proc macro printing a forged record) proving the forgery cannot reach cargo stdout, and record the result in ADR-037 — otherwise `validation_complete: true` is an unbacked claim on this path.

---

### P2

**2. Cold-compile budget makes the two timeout scenarios structurally tight → gate flakiness.**
`test_runtime.rs:150-159` uses `("timeout", TimedOut, 10)` and then asserts `result.stdout.contains("R2_DESCENDANT_STARTED")` (`:226`), while the observation window is only 8s (`:195`). Per ADR-037:18-20 the 10s wall covers preflight + source transfer + full cold compile *and* the sleeping test. Same shape at `inspection_runtime.rs` case 7 (`json!({"timeout":5})`), which additionally asserts `build_succeeded == Some(true)` and the `ACTUAL_TEST_TIMEOUT_STARTED` marker — both require the container start, source transfer and a from-scratch 1-CPU compile to complete inside 5s, with no shared target cache across cases. Any compile-phase timeout flips `build_succeeded` to `None` and drops the marker, failing the assertion rather than the property under test. Suggest 15–20s for these two scenarios (still well inside the 120s worker deadline) and keeping the 8s observation window proportionally larger.

**3. `clippy::collapsible_else_if` on new code — direct risk to the "Clippy must not regress" constraint.**
`crates/mcp-server/src/stdio/testing.rs:370-376`: the `else` arm of the `has_log` branch contains only an `if`/`else`. Style group, warn-by-default. Collapse to `} else if build_succeeded == Some(true) {`. (The sibling branch at `:362-369` is fine — its inner `if` has `else` arms.)

**4. Fixed `-- --test-threads=1 --color=never` is passed to `harness = false` targets.**
`rust_gateway.rs` (+121..+128) appends the harness tail unconditionally. Custom harnesses that reject unknown argv exit nonzero, which surfaces as a legitimate-looking `failed`/"failed after its reported build phase". ADR-037:25-27 says custom harnesses "affect coverage" but not that they can be *made to fail* by the fixed tail. Worth one sentence in ADR-037 Consequences and in the tool description, since a client cannot distinguish it from a real test failure.

**5. `readOnlyHint: true` on a tool that executes arbitrary test binaries and doctests.**
`testing.rs:489`. Defensible on the host-effects reading (source RO, network none, container discarded) and consistent with `rust.check`, but R2 escalates from "build scripts + proc macros" to "all selected test binaries + doctests", and `readOnlyHint` is what many clients use to skip per-call confirmation. Confirm this is a deliberate parity decision and record it in ADR-037 rather than leaving it implicit.

**6. Log section framing is forgeable by test stdout.**
`inspection_runtime.rs` `verified_test_log` asserts the log starts with `=== stdout ===\n` and contains `\n=== stderr ===\n`. A test can print exactly `\n=== stderr ===\n`, so any consumer splitting on that delimiter mis-attributes stdout bytes as stderr. Inherited from Check/Clippy, but trivially reachable in R2. Either escape/length-prefix the sections or state in ADR-037 that section boundaries in the retained log are advisory and unauthenticated (the tool description already disclaims producer authentication, but not the framing).

**7. Two cheap verifications on the sealed argv.**
(a) `implementation_fingerprint()` gained `domain/src/test_run.rs` (good), but only domain option types are visible in the hunk — confirm the argv construction site (`rust_gateway.rs`) is itself covered, otherwise an argv change alone will not move the configuration fingerprint. (b) The TestProject block is the last mutation of `args` before `return`, so `--`/harness args are terminal today; a future generic append in `arguments()` would silently land in harness argv. A `debug_assert!` or a comment at the block would pin that invariant, which the unit test at `rust_gateway.rs` `test_command_seals_harness_args_and_filter_position` currently pins only for the two selections it exercises.

---

### Verified clean (no finding)
- Filter grammar (`test_run.rs:42-56`) rejects empty/`>128`/non-ASCII/leading `-`/leading `:`/whitespace/`;`/`$()`, and the MCP mirror (`testing.rs:44-48`, `schemas.rs:12-16`) matches byte-for-byte; the filter can therefore never be read as a flag in its positional slot.
- Timeout `1..=60` enforced in the domain, not only in the schema; `timeout * 1000` cannot overflow.
- `test_output` gating in `cargo_run` (+299..+309) leaves Check/Clippy/Format `validation_complete` bit-identical; `inspect()` retains the 30s wall.
- `encode_bounded` downgrades `Passed → Failed` and clears `validation_complete` when diagnostics are dropped, and the 512KiB fallback re-encodes through `output(Err(OutputLimit))`.
- Cancellation/EOF integration test genuinely proves activity before cancelling (`active_slow_test` matches the exact sealed command string with `--no-trunc` and requires a live `/work/target/debug/deps/app-*` process), asserts the pre-20s window against the 30s gateway timeout, and re-admits only after observed object cleanup.
