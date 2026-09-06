## 1. Verdict

**Revise** — narrowly, and for documentation only. Every blocking finding from both final reviews is genuinely fixed and the new evidence is real. What remains is a P2 internal contradiction in the closure records themselves (they still say ADR-064/065 are `Proposed` after D03 accepted them), plus stale receipt citations. No code, test or gate work is outstanding.

## 2. Prior findings

| Finding | Status | Evidence |
|---|---|---|
| **VF-01** (P2) public docs deny Tasks is advertised | **resolved** | `SECURITY.md:326`, `README.md:439`, `CHANGELOG.md:24`, `docs/architecture.md:300,335`, `docs/implementation-status.md:23` all now say advertised, gated on peer declaration — matching `crates/mcp-server/src/stdio.rs:64` |
| **VF-CONTRACTS P2** only nextest proved `CreateTaskResult` | **resolved** | `crates/mcp-server/tests/inspection_runtime/tasks.rs:425,449,487`; selections present and passed in `docs/validation/M3-runtime.json:592,619,646` |
| **VF-02** (P3) counts vs receipt | **partially resolved** | Normalized to 62/62 in `README.md:415`, `CHANGELOG.md:10`, `docs/tools.md:903`, `implementation-status.md:27`, `M3-matrix.md:8`, `M3-07.md:23`; receipt genuinely holds 62 selections / 62 passed. Stale remainders below (new finding 3) |
| **VF-03** (P3) stale ADR index | **resolved** | `docs/adr/README.md:75-80` |
| **VF-04** (P3) budget samples described as freshly measured | **resolved** | `docs/validation/M3-02.md:47-49` discloses the carry-forward |
| **VF-05** (P3) Inspector cancel/Resource cells | **resolved** | `M3-02.md:73` with footnotes at `78-81` naming the non-zero one-shot CLI exit |
| **VF-06** (P3) transient `unavailable` undocumented | **resolved** | `docs/tools.md:1030-1031` |
| **VF-07** (P3) one-way test hook, no unadvertised oracle | **still open (accepted)** | `stdio.rs:66-72` — the hook can still only force `true`, and with `TASKS_ADVERTISEMENT_READY = true` it is now a no-op. W6 records this as an accepted gap |
| **VF-08** (P3) `revalidate` returns authorized on lock contention | **partially resolved** | `crates/mcp-server/src/stdio/tasks.rs:528-532` now documents the rationale; still no test — grep for `WouldBlock`/`contention`/`revalidat` in `crates/mcp-server/tests` returns nothing |
| **VF-09** (P3) no completed gate receipts | **resolved** | `M3-core-gate.json` 14 stages all `passed`; `M3-full-gate.json` 25 stages all `passed`, `source_inputs_unchanged: true` at line 7042; `audit-data` passed in 43.228 s (`:1186-1195`) with the unmodified `scripts/test-audit-data.py` command |

**Are the new tests real oracles?** Yes. `assert_created_task_envelope` (`tasks.rs:53-76`) pins `ttlMs`, `pollIntervalMs`, `createdAt == lastUpdatedAt` and explicitly asserts `content`/`structuredContent` are **absent** — a synchronous result smuggled through the same slot fails. W6's negative mutation (deleting the coverage arm → `-32603`) is a genuine kill. The mutation case additionally covers `auto`, correctly justified by the 300 s budget floor. `quality_artifact_cli.rs` drives the shipped binary with literal expectations (`:227-255`), including exit 1 / `recovery_required`.

## 3. New findings

**N-01 (P2) — closure records contradict the ADR files on G2.** `M3-matrix.md:25` ("ADR-064 y ADR-065 continúan formalmente **Proposed**"), `:32`, `M3-07.md:4,134,135,268,278` and `implementation-status.md:166-167,172-173,258` all record ADR acceptance as outstanding and G2 as In progress/blocked, while `docs/adr/ADR-064…:5`, `ADR-065…:5` and `docs/adr/README.md:79-80` state "Accepted 2026-09-06 by the M3 orchestrator". W6 edited those ADRs (count fixes) after D03 accepted them but wrote the matrix as if they were still Proposed. G9 therefore lists a prerequisite that the evidence says is already met.

**N-02 (P3) — G6 receipt cites an oracle that never ran.** `M3-06-rollback.md:36` and `:53-55` name `unsupported_platform_rejects_before_any_effect` as a proving/reused oracle, but it sits under `#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]` (`crates/project-adapter/tests/quality_artifact_store.rs:15`) and so is compiled out on the macOS-arm64 host every W6 gate ran on; it is not among the 10 selections in `M3-06-rollback.json`. The same doc says "6 new + 4 reused" (`:14`, matching `M3-07.md:52`) yet lists five reused oracles. The clause itself is still proved by `a_future_sibling_store_and_the_m2_journal_are_never_read_migrated_or_removed`, so this is a citation defect, not a coverage hole.

**N-03 (P3) — stale receipt citations survive.** `M3-04.md:39-40` and `M3-05.md:41-43` call `M3-runtime.json` "el recibo actual" at **55/55** with SHA-256 `910c4f82…`; that file now contains the 62-selection W6 run (timestamps `20:13:10`–`20:34:05` matching the full gate's `m3-runtime` stage), so both the count and the hash are wrong for the live file. `ADR-065:137` cites the same live file as "(W3, 55/55 after V-SEC)" while `:13` says 62/62. `ADR-062:9,751` still say 55/55. This is the residue of VF-02 in files D03 was told not to touch.

## 4. Required before Done

1. **N-01** — reconcile G2. Either move G2 to Done in `M3-matrix.md:25,32`, `M3-07.md:4,134,135,268,278` and `implementation-status.md:166-167,172-173,258`, or revert the ADR headers. The repo must not assert both.
2. **N-02** — drop `unsupported_platform_rejects_before_any_effect` from `M3-06-rollback.md:36,53-55`, or mark it explicitly as not executed on this platform, and make the reuse list agree with the receipt's four.
3. **N-03** — fix the count/hash in `M3-04.md:39`, `M3-05.md:41`, and disambiguate `ADR-065:137` / `ADR-062:9,751` as historical.
4. Optional, not blocking: record VF-07 and VF-08 as named accepted gaps in `M3-matrix.md` rather than only in the W6 delivery report, so they survive into the milestone record.

None of these require re-running a gate: all four are edits to Markdown that is not in the gate's `source_inputs` hash-checked set of behaviour, though re-hashing the docs inventory afterwards would be prudent if the closure claims `source_inputs_unchanged` against these bytes.

## 5. Limitations

Read-only pass; no commands. I could not recompute any SHA-256, re-run any gate, or confirm the receipts came from the binaries they claim — I verified internal consistency instead (stage timestamps in `M3-full-gate.json` bracket the standalone `M3-runtime.json` and `M3-rust-security.json` runs, and the counts match by enumeration: 62 selections/62 passed, 20/20, 10/10, 14 and 25 stages). I did not re-audit areas outside my prior findings and the three delivery reports. The three new materialization selections prove creation plus a clean cancelled terminal, not a full task-mode run of coverage/SemVer/mutation — correctly disclosed in `M3-07.md`. I did not verify the `check-architecture.py` hook-symbol rule beyond confirming VF-07's production-code condition is unchanged.