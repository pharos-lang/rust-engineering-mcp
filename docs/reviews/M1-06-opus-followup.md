# M1-06 independent follow-up

Claude Code2.1.259, explicit/actual claude-opus-5 High, read-only tools disabled.

## Verdict on the P1

**Closed for the confirmed attack path.** In the Docker fixture the forged `build-finished:success=true` lands at a clean line boundary, so the loop selects it as the boundary and parses `stdout[..offset]`; the genuine cargo tail (`compiler-message` for E0308, then `build-finished:success=false`) still deserializes, so this branch fires:

```rust
if stdout[offset..].lines().any(|line| serde_json::from_str::<Event>(line).is_ok()) {
    parsed.complete = false;
    parsed.build_finished = None;
}
```

That matches the reported `complete=false` / `build_finished=None`. Note the `!success && !tail.is_empty()` branch is *not* what saves this case (the forged record says `success=true`) — the Event scan is the sole load-bearing control, which is why the residuals below all target it. Slicing is panic-safe: `offset` accumulates `split_inclusive` line lengths, so it is always a char boundary; the unterminated-final-line `break` correctly prevents a torn trailing forgery from being accepted as the boundary.

The fix rests on one unstated invariant: **cargo always emits at least one further parseable `Event` after any forgery that occurs during compilation.** Findings R1/R2 are the places that invariant does not hold.

## P0
None identified in the reviewed code.

## P1

**R1 — The tail scan detects only *parseable* Events, so an interleaving writer can hide the genuine build-finished and re-open the exact forged-evidence hole.**

Evidence: `any(|line| serde_json::from_str::<Event>(line).is_ok())`. A tail line that is corrupted is `is_err()` and therefore **not** flagged.

The same proc-macro that the fixture proves can write to the shared stdout fd can (a) write the forged `build-finished:success=true` as one atomic ≤PIPE_BUF write, landing on a line boundary, then (b) leave a detached child spinning on newline-free writes. Cargo's remaining stdout lines then arrive as `<garbage>{"reason":"compiler-message",...}` — unparseable, so the scan sees no Event, `complete` stays `true` and `build_finished` stays `Some(true)` on a build that actually failed. The window is small (typically 2 lines, `--jobs=1`) but it is a repeatable race, not a one-shot.

Blast radius is bounded by the exit code: per ADR-037 (“Exit0 plus complete successful build is passed”), a failed compile still exits 101, so this cannot manufacture `passed`. It manufactures `build_succeeded: true` + `validation_complete: true` alongside a failure — i.e. exactly the false “build fine, tests failed” evidence the P1 fix exists to prevent, which points an agent at the wrong layer.

Suggested hardening (keeps `running 0 tests` and `not JSON` accepted, kills the splice deterministically): treat the tail as ambiguous if a line parses as an `Event` **or** contains a cargo record marker, e.g. `line.contains("\"reason\":")`. Optionally corroborate with exit status in the gateway (`build_finished == Some(true)` with exit 101 is self-contradictory).

I flag this as P1 because it reproduces the confirmed falsehood against the newly added control, and the fix is one predicate. If you weight exploitability over class, it is a top-of-P2 — but the cost asymmetry makes that distinction academic. This does contradict “No unresolved confirmed P0/P1 after the fix” in `docs/reviews/M1-06-principal-disposition.md`; the *confirmed* fixture bug is genuinely closed, the class is not.

## P2

**R2 — `build_finished` survives as `Some(_)` on paths where `complete` is false.** Only the Event-scan branch clears it. Two reachable paths keep a value:
- `stream_complete == false` with a clean prefix ending in `Finished{success:true}` and a benign tail → `complete=false`, `build_finished=Some(true)`. Your own test exercises this and asserts only completeness: `assert!(!checked(parse_test(&(prefix.clone() + tail), &source()?, false))?.complete);`
- `!success && !tail.is_empty()` → `complete=false`, `build_finished=Some(false)`.

Neither is exploitable for a *success* claim on its own, but a truncated capture plus a forged early `Finished(true)` yields a `build_succeeded: true` field that only the consumer's `validation_complete` gate suppresses. Recommend making it structural rather than caller-dependent: `if !parsed.complete { parsed.build_finished = None; }` before `return Ok(parsed)`, and stating the rule in ADR-037 (“build_succeeded is never reported when validation_complete is false”) — the ADR currently only requires clearing on the tail-event path.

**R3 — Prefix diagnostics on the ambiguous path are attacker-authored but still returned.** `parse(&stdout[..offset], ...)` keeps everything before the forged record, and an adversary that can forge `build-finished` can equally forge `compiler-message` records. This is consistent with the existing partial-diagnostic policy (`test_compile_failure_...` asserts `diagnostics.len() == 1` with `complete=false`), and span/rendered redaction limits it to content the attacker already supplied — so it is presentational, not a leak. Confirm the tool response and `docs/tools.md` state that `diagnostics` are compiler-attributable only when `validation_complete` is true; I could not check those files this turn.

**R4 — False-ambiguity has concrete benign triggers, including self-hosting.** Two asymmetries:
- Any cargo-looking line printed by a *passing* build's harness collapses a green run to incomplete. This repo is a likely victim: its own fixtures emit literal `{"reason":"build-finished","success":true}` (`finished()` in the pasted tests). With `--test-threads=1` libtest prints captured stdout for failing tests, so a failing diagnostics test flips the outcome from `failed` to `incomplete`, which is the less actionable result.
- For failed builds, `!success && !stdout[offset..].is_empty()` treats *any* trailing byte as unexplained — a proc macro with a stray `dbg!`/warning turns a legitimate E0308 result into `incomplete`, masking diagnostics that are in fact genuine.

Both are fail-closed and deliberate per ADR-037, so I would not change behaviour; I would document the second trigger in `docs/tools.md` so the “incomplete” outcome is diagnosable by users.

**R5 — Test gaps that let R1/R2 regress silently.** No case asserts `build_finished` on the paths that do not clear it (both the `stream_complete=false` and `prefix + "unexpected tail\n"` assertions check `complete` only). No case covers a corrupted-but-cargo-shaped tail line, e.g. `lines(&[finished(true)])? + "garbage{\"reason\":\"build-finished\",\"success\":false}\n"` — today that returns `complete=true, build_finished=Some(true)`, which is R1 in one assertion. Also unpinned: that `Event` stays permissive enough to match unknown `reason` values (`"{\"reason\":\"compiler-artifact\"}"` covers this incidentally, but nothing prevents a future `deny_unknown_fields` from silently narrowing the whole scan).

## Verification limits

No tools were used, per your instruction, so: the module path for `parse_test` is inferred from the diff surface, not confirmed; `parse`, `Event` and `MAX_INPUT` semantics are inferred from the pasted tests; and I did not verify the 393-test/10-stage count, the final 13-test serial Docker gate, or the fixture cleanup — those remain your open items and R1 does not depend on them.
