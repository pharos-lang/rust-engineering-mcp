All gates have exited, nothing is left running, and Docker is clean.

## Task
Make the M3 Docker-backed evidence real: clean the killed session's leftovers, diagnose the leaky-selection hang, run the complete 55-selection runtime gate and the 20-selection Rust security gate, and calibrate coverage.

## Result per gate

| Gate | Result | Receipt |
|---|---|---|
| Preflight (fmt/check/clippy/test) | **all exit 0**; workspace tests 1059 passed / 109 ignored across 82 binaries | `target/w1-logs/*.log` |
| M3 runtime (55 declared) | **47 passed / 48 executed**, 7 not executed (fail-fast), 937.6 s | `docs/validation/M3-runtime-attempt7.json` |
| — nextest cut | **19/19** (15 `nextest_runtime` + 4 `inspection_runtime::nextest_runtime`) | |
| — semver cut | **18/18** (16 `semver_runtime` + 2 `inspection_runtime::semver_runtime`) | |
| — mutation cut | **10/10** | |
| — coverage cut | **0/8** — first selection fails, remaining 7 not executed | |
| Rust security (20) | **20/20 passed**, 8m53s, on the ADR-065 bytes | `docs/validation/M3-rust-security.json` |

`docs/validation/M3-runtime.json` was **not** overwritten — there is no passing 55/55 run to write. The prior passing security receipt is preserved at `M3-rust-security-pre-adr065.json`.

**catalog_sync answer:** the five tests another session reported as `EPERM` **pass here** — `catalog_sync::tests::*`, 6 cases, 0 failures. That failure was the Codex sandbox, not the code.

## Leftovers removed
Exactly two, both belonging to killed job `03124ad0…`: container `rust-mcp-cargo-03124ad07b418d9f0310c0945803ac94` (exited 0) and volume `rust-mcp-source-03124ad07b418d9f0310c0945803ac94`. The unrelated `tender_goldstine` (terraform-mcp-server) and three `erp-*` volumes were left untouched.

## Root cause of the leaky hang
The hang was **not** in the leaky nextest run — the leftovers are a single ingest + single `/source`-only `rust-mcp-cargo` container, which is the `execute_observed` calibration shape, not the nextest shape (no junit volume, no guardian).

Docker Desktop's backend log pins it exactly: calibration job `03124ad0…` started its run container at `10:35:24.086Z`, the daemon deregistered it at `10:35:26.551Z`, and **the host process never issued another Docker request** — while the daemon stayed healthy (25 heartbeat lines/minute) for the following 31 minutes. So the stall was host-side, in the window between `docker container start --attach` returning and the immediately following `container inspect`. It was not a cleanup/volume-profile defect: I audited `cleanup_inner_fallible`, `cleanup_nextest`, `cleanup_coverage_inner` and `mutation_gateway::cleanup_until_with_options` — every profile is matched to its volume and every control call is bounded at 10 s.

**It does not reproduce**: 3/3 isolated runs of that selection on the same bytes passed (16.4 s, 16.0 s, 16.0 s), plus a fourth pass inside the full gate. In that window the only unbounded primitive is waiting on a SIGKILL'd child the kernel has not reaped; bounding that would change already-qualified containment semantics, so I did not touch it.

**What I did fix** is that a stall could hang the gate unattended: `scripts/test-m3-runtime.py` now runs each selection in its own session under `RUST_MCP_M3_STEP_TIMEOUT_S` (default 900 s, vs. 124 s for the longest legitimate step), kills the whole process group on expiry, and records `timed_out: true` / `exit_code: -9` so the receipt closes `failed` instead of `running`. Verified for real with `RUST_MCP_M3_STEP_TIMEOUT_S=1`: recorded `failed`/`timed_out`/`-9` at 1.004 s. Inherent consequence, documented: a forced kill can't run the gateway's cleanup, so it may leave owned objects — the next gate then refuses to start (fail-closed). I removed the two objects my own probe left.

## Coverage: genuinely blocked, and not by the environment
ADR-065's executable target volume **works** — the instrumented phase compiled and *ran* the test inside it (`Running unittests src/lib.rs (/work/coverage-target/debug/deps/…)`, `test tests::one_arm_is_covered ... ok`, run exit 0). The original `noexec` blocker is solved.

But all three report phases fail identically (`target/m3-runtime-w1/47.log`):

```
error: failed to merge profile data: failed to create file
`/work/coverage-target/source-profraw-list`: Read-only file system (os error 30)
```

once per format. `cargo-llvm-cov` 0.9.0 writes `<crate>-profraw-list` and `<crate>.profdata` into `CARGO_LLVM_COV_TARGET_DIR` before exporting, and the pinned `cargo llvm-cov report` help exposes no option to redirect that merge (`--output-path`/`--output-dir` only control the final report). **ADR-065's own access-matrix row "each `CoverageReport`: `/work/coverage-target` read-only" is not implementable with the approved binary.** The gateway fails closed correctly — run exits 0 with all three artifacts absent, and the test fails at `coverage_runtime.rs:145`.

Minimal remediation, **not applied** — widening sandbox write access is an ADR decision, not a validation one: make `/work/coverage-target` read-write in the three `CoverageReport` phases only (one line in `Phase::coverage_target_writable`), keeper and export unchanged. This needs your authorization.

## Calibration table
No JSON/LCOV/HTML were produced, so **there is no coverage calibration and I invented no numbers**. `fixtures/coverage/README.md` contains no hand-derived expectations to compare against, so I recorded the derivation in `M3-03.md` as an explicit hypothesis that no test asserts:

| Quantity | Hand derivation | Status |
|---|---|---|
| Functions | `covered` + `tests::one_arm_is_covered`, both executed | 2/2 expected, unobserved |
| Regions | function body + `if`/`else` arms; only the `true` arm runs | denominator ≥3 with ≥1 uncovered, unobserved |
| Lines | all executable lines run; `if/else` is one line | 100% expected, unobserved |

The fixture's point is that contrast (100% lines, incomplete regions). llvm-cov identity is proven only indirectly: the pre-run identity check passed (a mismatch returns `Unavailable` before any object is created).

## Files changed (SHA-256)
```
2bc79d64ba4917bc7717be0620a765a5338bc8f98cc83c0010522ecd4b8138c8  scripts/test-m3-runtime.py
b3508b454f4505fa0323c0f3d5cab3cf80fef34620cd0c2c1db1fd1978e2f0ab  docs/validation/M3-03.md
acb0f081b4d9b7ce233b142babe4826c9c8d3c46cc74d4b135750c281deb6451  docs/validation/M3-matrix.md
5615e019d8834238f9050cbe249db62487a3aa3a4581882a0e0a988e9b99c496  docs/tools.md
97a863a2c4dd09e148d16aa19aee006964bc7f2ce28e3a492820b369d36a1e34  docs/validation/M3-runtime-attempt7.json
cb5b7ad6c3fa729ac971b269e3c696c4adc45f7f50bfde52c2e19c9bcd836808  docs/validation/M3-rust-security.json
0e50923acc9b631186d44f5dc87834fabd76caa3fffbf08be4d51a8794bbf588  docs/validation/M3-rust-security-pre-adr065.json
```
No Rust source was modified. `cargo fmt --check` exit 0 and `scripts/check-architecture.py` still pass; attempt7's recorded harness hash matches the file on disk (verified).

## Tests executed
- `cargo fmt --check` / `check` / `clippy -D warnings` / `test --workspace --locked --offline` — exit 0; 1059 passed, 109 ignored.
- `cargo test -p rust-engineering-execution --test nextest_runtime leaky_… -- --exact --ignored` ×3 — exit 0, 1 passed each, 16.4/16.0/16.0 s.
- `python3 scripts/test-m3-runtime.py` (`RUST_MCP_M3_RUNTIME_OUTPUT=target/m3-runtime-w1`) — exit 1, 47/48, 937.6 s → `docs/validation/M3-runtime-attempt7.json`.
- `python3 scripts/test-rust-execution.py` — exit 0, 20/20, 8m53s → `docs/validation/M3-rust-security.json`.
- Step-bound probe `RUST_MCP_M3_STEP_TIMEOUT_S=1` — exit 1, `timed_out`/`-9` at 1.004 s.

## Docker hygiene
Checked before and after every gate. Final state:
```
$ docker ps -a --filter label=org.rust-mcp.execution=true --format '{{.ID}} {{.Names}} {{.Status}}'
$ docker volume ls --filter label=org.rust-mcp.execution=true --format '{{.Name}}'
(both empty — zero owned containers, zero owned volumes)
```

## Risks
- The leaky-selection stall is unexplained and unreproduced; it can recur. It is now bounded to a recorded failure rather than an unattended hang, but the underlying host-side wait is untouched.
- The step bound is 900 s; a legitimately slower future step would be misreported as a stall.
- Re-running the gate after the coverage fix will change the configuration fingerprint again, so attempt7's 47 passes will need re-earning — they are not transferable to the amended bytes.

## Decisions
- Did not widen `/work/coverage-target` to writable in report phases — outside ADR-065's authorized shape.
- Did not bound the post-SIGKILL child wait in `supervisor.rs` — that would alter qualified containment semantics.
- Recorded the known-counts derivation in `M3-03.md` rather than `fixtures/coverage/README.md`, to avoid invalidating the receipt's configuration hashes.
- Did not touch `scripts/test-rust-execution.py` (outside my ownership), so it still has no per-step bound.

## Open issues
1. **Blocking on you:** authorize (or reject) the one-cell ADR-065 amendment. Without it, coverage cannot produce a report and M3-03 cannot be qualified — the remaining 8 selections and the entire coverage calibration depend on it.
2. `docs/validation/M3-runtime.json` still holds the old failed attempt-4 content; it stays that way until a 55/55 run exists.
3. The coverage timeout/cancellation cases have still never executed — they sit behind the fail-fast stop, contrary to what `docs/tools.md` previously claimed (I corrected that statement).