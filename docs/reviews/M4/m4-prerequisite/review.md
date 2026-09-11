# Independent review — post-review M3 delta as M4 prerequisite

**HEAD `c66a3704e1ad290603a3c1d10413df90d15c2b03` · baseline `e2ec7da` · package `docs/reviews/M4/m4-prerequisite/`**
Read-only. No files edited, no commands run, no delegation. No M4 implementation exists in the tree and none was reviewed.

## 1. Verdict

**Revise** — narrowly, on one P2 that touches a required gate. Everything in the product delta is Accept-grade.

The 47-file delta is, in the code paths that matter, **behaviour-preserving**. I checked the gateway mount/phase verification predicates field by field against what they replaced, the single-flight and runtime re-check consolidation call site by call site, the container start/OOM-inspection consolidation argument by argument, the mutation filesystem lock change, and the stdio worker/clock/operational consolidation. I found no P0 and no P1. The public contract is unchanged: **not one of the 23 tool/doctor snapshots in `crates/mcp-server/tests/snapshots/` appears in the delta**, and `crates/mcp-server/tests/protocol.rs:402-429` asserts full JSON equality of all 22 tool definitions against those files through a real server session.

The single blocking item is **P2-1**: the SonarCloud coverage-exclusion list grew from 4 to 28 entries and now exempts ten files that *do* run portable unit tests on the Linux scanner, the `docs/ci.md` justification for them is factually false, and the file that configures a required check is not in the gate's hash-checked inputs. It is cheap to fix: `sonar-project.properties` and `docs/ci.md` are **not** among the 810 `source_inputs` of `docs/validation/M3/full-gate.json`, so correcting them invalidates no receipt and requires no gate re-run.

## 2. Method and the manifest limitation

`docs/reviews/M4/m4-prerequisite/inputs.json` binds each of the 47 paths to a byte count and a SHA-256. **I cannot recompute either with Read/Grep/Glob, and I did not.** What I did instead: read `delta.patch` end to end, then open the current tree for every security-relevant hunk and confirm the tree matches the patch. That corroboration held for `coverage_gateway.rs`, `mutation_gateway.rs`, `rust_applied.rs`, `rust_gateway.rs`, `mutation_test_gateway.rs`, `stdio/clock.rs`, `stdio/operational.rs`, `stdio/workers.rs`, `stdio/coverage.rs`, `filesystem/macos/mutation.rs`, `Cargo.toml` and `sonar-project.properties`. I state no claim about the hashes.

Where I could verify receipts by reading alone, I did, and they hold:

| Claim | Verified how |
|---|---|
| full gate 810 inputs | `M3-full-gate.json` contains exactly 810 `"mode":` entries under `source_inputs` |
| full gate clean | no `"status": "failed"` and no `"skipped"` anywhere in `M3-full-gate.json`; `source_inputs_unchanged: true` at `:7114` |
| runtime 62/62 | `M3-runtime.json` has 63 `"status": "passed"` (62 selections + envelope), `:4` `16:26:11Z` → `:3369` `16:46:40Z` = 20m29s, matching the 1,229.210 s `m3-runtime` stage in `M3-07.md:616` inside the W9 full-gate window |
| rust-security 20/20 | `M3-rust-security.json` has 21 `"status": "passed"` |
| version bump is only that | `delta.patch:1-89` — 8 workspace package `version` lines in `Cargo.lock` plus `[workspace.package] version`; no third-party resolution changed. `Cargo.toml:16` is `0.3.0-dev` in the tree |
| no test pins the version literal | `0.2.0-dev` appears in 30 files, **none** under `crates/` |

## 3. Findings

### P0 — none
### P1 — none

### P2-1 (blocking) — the required SonarCloud gate no longer measures coverage on ten files that do execute portable tests, and the stated justification is false

**Where.** `sonar-project.properties:28` (the list), `:11-15` (the rationale comment), `docs/ci.md:46-56` (group 3) and `:68-73` (group 5). Patch hunk `delta.patch:5717-5730`.

**Mechanism.** `sonar.coverage.exclusions` went from four maintainer-only Python scripts to 28 entries, adding whole Rust files. `docs/ci.md:50-52` justifies group 3 with «los puertos reciben `&RustGateway` concreto, así que sin daemon no hay ruta que un test portable pueda tomar». That is true for exactly one of them. It is false for these, each of which carries a plain `#[cfg(test)] mod tests` with no target gate and therefore runs on the Ubuntu scanner:

`crates/execution-adapter/src/mutation_gateway.rs:963`, `mutation_test_gateway.rs:398`, `nextest_gateway.rs:416`, `coverage_gateway.rs:334`, `resolution_gateway.rs:998`, `project_inspection.rs:927`, `nextest_port.rs:191`, `mutation_test_port.rs:200`, `lib.rs:636`, and — under group 5 — `crates/mcp-server/src/stdio.rs:1145`.

By contrast `rust_gateway.rs:2311` is `#[cfg(all(test, target_os = "macos"))]`, so for that file the justification is accurate. `rust_applied.rs` is correctly **not** excluded.

This is not hypothetical for this delta: `mutation_gateway.rs:1092 mutation_volume_requires_exact_tmpfs_quota_identity_and_ownership` is a portable test that exercises `parse_volume_with_options`, the exact function this delta routed `create_tmpfs_volume` through (`mutation_gateway.rs:157-185`). `coverage_gateway.rs:334` tests `validated_html_archive`, the tar-egress validator. `stdio.rs` holds the Tasks advertisement gate (`:66-73`) and the dispatch arms whose deletion M3-07 §W6 cites as a proven negative mutation.

**Why it matters for M4 specifically.** SonarCloud is one of the five contexts `main` requires. Its "Coverage on New Code" condition cannot fire on lines added to an excluded file. `rust_gateway.rs`, `mutation_gateway.rs` and `stdio.rs` are precisely where M4 gateway and transport work will land. From this commit forward, new product code in those files carries no coverage obligation on a required check, silently.

**Compounding.** `sonar-project.properties` is absent from the gate's 810 `source_inputs`: grepping `M3-full-gate.json` for `sonar` yields exactly one hit, `.github/workflows/sonarcloud.yml` at `:1493`. W7 records the same property for `docs/` (`W7-requalify-final/last-message.md:57`). So the file governing what a required check measures can be widened again without invalidating any receipt or tripping `source_inputs_unchanged`.

**Discriminating oracle.** Add a trivially uncovered portable function to `crates/execution-adapter/src/mutation_gateway.rs` and to `crates/execution-adapter/src/rust_applied.rs` on a scratch branch and run the SonarCloud job: New Code coverage drops only for the second. Cheaper and offline: `coverage/rust.lcov` (produced by `cargo-llvm-cov` before analysis, per `docs/ci.md:17-19`) already contains `SF:` records with hit counts for `mutation_gateway.rs`; compare them against what Sonar reports as measured lines for that file.

**Recommended fix (no gate re-run needed).** Prefer narrowing over correcting prose: move the daemon-only phase construction out of the files that also hold portable logic, or simply drop the ten files above from `sonar.coverage.exclusions` and let the real number stand. If the owner instead accepts the current metric scope, then `docs/ci.md:50-52` and `:68-73` must be corrected to say what is actually true — that whole files with executing tests are excluded and why that is acceptable — and `sonar-project.properties` should be added to the gate inventory so the next widening is visible in a receipt. Either way, neither file is in the 810 inputs, so **core 14/14, full 25/25, runtime 62/62 and rust-security 20/20 remain valid over unchanged bytes.**

### P3-1 — `operational::unavailable` is not exhaustive, so a future operational code silently reports `blocked` instead of `unavailable`

**Where.** `crates/mcp-server/src/stdio/operational.rs:19-24`; test at `:83`.

**Mechanism.** The code this replaced (`delta.patch:3244-3254` for nextest, `:2692-2702` for mutation) was an exhaustive `match code` producing `(Code, bool)` in two places, so a tenth `OperationalErrorCode` variant was a compile error twice. Now `Code::from` (`nextest.rs`, `mutation_test.rs`) stays exhaustive, but `unavailable()` uses `matches!`, which classifies any unlisted variant as `blocked`. A future variant meaning "the approved runtime is absent" would therefore reach the peer as `status: blocked` — "this host refused a request it understood" — instead of `unavailable` — "the request was never assessed". That is a wire-visible contract error, not an internal one.

**Oracle gap.** The test at `operational.rs:83` enumerates the nine current variants by hand, so it would not fail either.

**Fix.** Replace the `matches!` with a `match` that spells out all variants on both arms; the compiler then blocks any addition until the classification is decided.

### P3-2 — the live runtime receipt hash is stale in four places (same class as the previously-closed N-03)

**Where.** `docs/validation/M3/04.md:30` and `:42`; `docs/validation/M3/05.md:27` and `:44` — all four call `M3-runtime.json` the «corrida vigente» and give `5cfab6c56eaa9d6ab0f306b4920eb61eea21cd4cbc371bf7199c7094a674bf25`. `docs/validation/M3/07.md:604` records the live file as `0c2984052db8a8849be32f024002f86b381c3ee69679fb098fe50901f7cb3e0a` after W9 re-qualified on `0.3.0-dev` bytes and preserved the earlier receipt as `M3-runtime-v0.2.0-dev.json` (which exists). `M3-07.md:207-209` likewise links the live filenames `M3-runtime.json` / `M3-rust-security.json` with the superseded W7 hashes `5cfab6c5…` / `4ad812a0…` without marking them historical, unlike the explicit `-preS03` and `-v0.2.0-dev` preservation lists elsewhere in the same file.

This is a documentation discrepancy, not a defect: the live receipts themselves are internally consistent (§2). It is the exact finding VR-opus raised as N-03 and VC-confirm closed; the two later re-qualifications reintroduced it. **Oracle:** `shasum -a 256 docs/validation/M3/runtime.json` must equal what `M3-04.md:30` claims. **Fix:** update the four citations to `0c298405…`, and mark `M3-07.md:207-209` as the W7 run superseded by §W9.

### P3-3 — the fork/flock barrier cites an ADR that does not contain the claim, and covers only one of the two stores

**Where.** `crates/project-adapter/src/filesystem/macos/mutation.rs:262-269` (comment), `:255-315` (barrier), `:396-407` (`StateRoot::lock`).

The mechanism and the fix are correct. `StoreLock` declares `_file` before `_fork_barrier`, so the flock fd is dropped before the read guard — the right order — and `ForkBarrier::enter()` runs before `open_lock`, so the whole window is covered; the reentrancy counter correctly avoids the nested-read deadlock. In non-test builds the type is a one-field wrapper with no behaviour change.

Two accuracy problems. First, `:266` asserts «ADR-061 records the same effect for the quality artifact store», but `docs/adr/ADR-061-private-quality-artifact-store.md` says nothing about fork-inherited descriptors; its only lock text is `:33`, which describes contention as a bounded busy rejection. Second, the barrier exists only for the mutation store. `crates/project-adapter/src/filesystem/macos/quality.rs:352` takes an identical `flock` with no barrier, while `crates/project-adapter/tests/quality_artifact_store.rs` both takes flocks (`:1569`, `:1987`) and spawns processes — the same harness flake class remains open there.

**Pre-existing, not a regression**; the delta fixed the store that was actually flaking. **Fix:** either add the claim to ADR-061 or drop the citation, and extend the barrier to `quality.rs::lock` before that harness starts flaking too.

### P3-4 — the consolidation is incomplete, and the shared module's stated invariant is not true repo-wide

**Where.** `crates/mcp-server/src/stdio/workers.rs:433-446` is the new shared `joined_result`. `toolchain.rs:264-279` and `explaining.rs:313-328` still carry byte-equivalent private copies, while `check.rs`, `clippy.rs`, `format.rs`, `testing.rs`, `nextest.rs`, `mutation_test.rs` and `semver.rs` were switched over. Separately, `operational.rs:1-8` says the reading is «decided once instead of once per vertical», but two verticals still decide it themselves and the module doc does not say so: `coverage.rs:569-581` maps `WorkerError::Busy` to `InspectionError::Internal` and lets any body error win over the interrupting signal, and `quality.rs:714-729` lets `Ok` win unconditionally (with a good in-place comment explaining the publication commit point).

**Concrete consequence.** For `rust.coverage`, a body that reports its own cancellation while cleanup was interrupted by a deadline is surfaced as `cancelled`, whereas the same situation on `rust.test.nextest` is surfaced as `COMMAND_TIMEOUT`. Under ADR-060 a `cancelled` terminal is only legitimate after observed cleanup, so coverage understates that case. It is pre-existing and now deliberately pinned by the new test at `stdio/coverage/tests.rs` (`delta.patch:2255-2293`), so it is a consistency and documentation issue, not a new defect. **Fix:** point `toolchain.rs` and `explaining.rs` at `workers::joined_result`, and record in `workers.rs`/`operational.rs` why coverage and quality diverge.

## 4. What I verified as sound (no finding)

- **Applied-container verification** (`rust_applied.rs:88-227`). `no_host_authority`, `applied_limits_ok`, `applied_profile_ok`, `only_created` and `sorted_env` reproduce the three previous inline predicates field for field, including the tmpfs count checks that stayed at each call site (`:436` generic, `2 + extra_tmpfs`; mutation `3 if Fix else 2`; resolution `2`). The oracle is real and discriminating: `mutation_security_changes` (`:1455-1539`) mutates 70 distinct pointers — every field moved into the shared predicates, including `Image`, `WorkingDir`, `LogConfig`, `RestartPolicy`, `Sysctls`, `Ulimits`, `MaskedPaths`, `ReadonlyPaths`, `MemorySwap`, `ShmSize`, `CgroupnsMode` — and `:1562-1599` asserts each one is rejected across all five mutation phases.
- **Single-flight and runtime re-check** (`rust_gateway.rs:827-838`, `:845-869`). Five call sites keep the identical order: busy lock → quarantine/calibrating/verified → cancel → executable re-digest → engine identity. `mutation_gateway.rs:731-753` deliberately keeps its own inline copy (it checks `verified` only, not `calibrating`) — pre-existing and unchanged.
- **Container start consolidation** (`rust_gateway.rs:1662-1699`). Each of the five call sites passes exactly the `interactive` flag and `output_limit` it passed before (`phase.ingesting()` for `phase()`, `false` elsewhere; `MAX_JUNIT_EXPORT`, the coverage export cap, and `mutation_test_gateway::output_limit` preserved). Argv is identical. The `Stop::Exited` → re-inspect → `completed(code)` → `oom_killed` path is unchanged. Cleanup (`cleanup_coverage_with_target`, `cleanup_inner_fallible:1611-1653`) is untouched by the delta.
- **Volume creation** (`mutation_gateway.rs:157-185`). `create_tmpfs_volume` ends in `parse_volume_with_options`; the old `mutation_test_gateway::output_volume` ended in `parse_volume`, which is `parse_volume_with_options(..., VOLUME_OPTIONS)` (`:187-193`). Identical. Coverage's second volume correctly passes `COVERAGE_TARGET_VOLUME_OPTIONS` and `verify_coverage` (`rust_applied.rs:273-281`) still requires both volumes to be exact.
- **Public contract.** No snapshot changed; `ExecutionModeDto` (`nextest.rs:105-112`) is derive-for-derive identical to the three copies it replaced, including the type name schemars uses for `$defs`.
- **New tests pin independent expectations.** The added suites assert literal wire values, not values re-derived from the code under test: `nextest.rs:1046+`, `mutation_test/tests.rs:350+`, `coverage/tests.rs`, `semver/tests.rs`, `tasks/tests.rs:810+`, `project-adapter/tests/mutation_digest.rs`. The digest test asserts collision-freedom across kind, before, after, validation provenance, declared directories and a path/byte shift — a genuine length-prefix oracle, not a tautology.
- **`cargo_vendor.rs` mode constants** are pure renaming: `0o600 | 0o066 == 0o666` and `0o700 | 0o077 == 0o777`, the same values as before. No oracle weakened.
- **Probe path hardening** (`ProbeDirectory` in both `scripts/probe-m2-*.py`) resolves the base once and rejects any name whose resolved parent is not the base, which also closes symlink escape. Maintainer-only scripts; acceptable.

## 5. Known accepted M3 debt — carried forward unchanged, not regressions

I found no evidence that any of these changed, and I am not re-raising them:

1. **rmcp 3.2.0 silent JSON-RPC batch drop** (`crates/mcp-server/tests/protocol.rs:323`). Pre-existing in the pinned SDK at `rmcp-3.2.0/src/transport/async_rw.rs:182`; `Cargo.lock`'s `rmcp` pin is untouched by this delta. The `test` stage can still fail intermittently on that one case.
2. **One unreproduced Linux `cli` failure** (`crates/mcp-server/tests/cli.rs:131`). No root cause; no fix applied; the oracle is correct as written and should not be touched before an S03-style Linux reproduction.
3. **`test-hooks` advertisement override is one-way** (`stdio.rs:66-73`, with `TASKS_ADVERTISEMENT_READY = true` at `:66`). The unadvertised wire path still has no end-to-end oracle through the real binary.
4. **`LiveJobAuthority::revalidate` under contention** may answer `authorized`; sound only while the single-permit rule holds, still not asserted by a test.

## 6. Documentation discrepancies, separated from defects

- P3-2 above (stale runtime-receipt hash in four citations).
- P3-3 above (ADR-061 citation).
- The `docs/ci.md` half of P2-1.
- Non-issues I checked and cleared: `docs/compatibility.md:8,342-344` correctly distinguishes the `0.3.0-dev` checkout from the historical `0.2.0-dev` M2 qualification; `docs/implementation-status.md:23` links `M3-04-semver-calibration.md` and `M3-05-mutation-calibration.md`, both of which exist; the four `-v0.2.0-dev` preserved receipts exist as W9 claims.

## 7. Files inspected

**Review package:** `docs/reviews/M4/m4-prerequisite/inputs.json`; `delta.patch` (all 5,730 lines).

**Product source (current tree):** `crates/execution-adapter/src/{coverage_gateway.rs, mutation_gateway.rs, mutation_test_gateway.rs, rust_gateway.rs, rust_applied.rs, lib.rs, nextest_gateway.rs, resolution_gateway.rs, project_inspection.rs, nextest_port.rs, mutation_test_port.rs, state.rs, rust_calibration.rs}` (targeted reads and structural greps); `crates/mcp-server/src/stdio.rs`; `crates/mcp-server/src/stdio/{clock.rs, operational.rs, workers.rs, coverage.rs, nextest.rs, quality.rs, toolchain.rs, explaining.rs, auditing.rs, catalog.rs, resources.rs}`; `crates/project-adapter/src/filesystem/macos/{mutation.rs, quality.rs}`; `crates/project-adapter/tests/{mutation_digest.rs, cargo_vendor.rs, quality_artifact_store.rs (grep)}`; `crates/mcp-server/tests/protocol.rs`; `crates/mcp-server/tests/snapshots/` (inventory).

**Configuration:** `Cargo.toml`; `sonar-project.properties`.

**Documents:** `AGENTS.md`; `docs/validation/M3/07.md`; `docs/validation/M3/delegation/{W7-requalify-final, VR-opus-rereview, VC-confirm}/last-message.md`; `docs/ci.md`; `docs/adr/ADR-061-private-quality-artifact-store.md`; `docs/validation/{M3-full-gate.json, M3-runtime.json, M3-rust-security.json, M3-04.md, M3-05.md}`; `docs/implementation-status.md`; `docs/compatibility.md`.

## 8. Limitations

- **No hashes, no sizes, no execution.** I could not recompute any SHA-256 or byte count, so the manifest binding in `inputs.json` and every receipt digest cited in `M3-07.md` are unverified by me. I substituted content corroboration against the tree and internal consistency checks (entry counts, status-string counts, timestamp arithmetic).
- **No gate, test, build or Docker run.** Every runtime claim — 62/62, 20/20, 25/25, 14/14, 10/10, 1,105 tests — is read from receipts, not reproduced. I have no independent evidence that the receipts were produced by the binaries they name.
- **The delta was reviewed, not the milestone.** I did not re-audit unchanged architecture, the seccomp profiles, ADR-060/061/062/064/065 conformance beyond the touched call sites, the catalog/semantic adapters, or anything outside the 47 listed files and their immediate callers.
- **`main` vs local tree.** I read the working tree at `/Users/cburgosro/Projects/rust-mcp`, which `git status` shows carrying six untracked `docs/` paths beyond `c66a370`. None is source; none is in the 810 gate inputs. I did not diff the tree against the commit.
- **Coverage claims in P2-1 are about which files Sonar measures**, established from the exclusion list and the `#[cfg]` gating of each file's test module. I did not read `coverage/rust.lcov` (not present in a clean tree) and make no claim about the resulting percentage.
