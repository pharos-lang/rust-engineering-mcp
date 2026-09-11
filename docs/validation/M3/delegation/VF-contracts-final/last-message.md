## VF-CONTRACTS — Independent contract review of M3 closure

**Verdict: Revise** (P2 doc-honesty contradictions plus a P2 test-coverage gap on the public contract surface; no P0/P1 found).

### What I verified as genuinely fixed since V-CONTRACTS/V-SEC
- **V-CONTRACTS F1–F4 are real fixes**: `coverage.rs` now routes through the shared `select_execution_mode` (identical to nextest/semver/mutation_test), `Code::TasksRequired` and `Code::ToolNotInstalled` are closed enum variants (not raw strings), and `coverage_contract_and_synchronous_gate_are_stable_in_all_wire_versions` now exists in `protocol.rs:1479` covering all five protocol versions with `TASKS_REQUIRED`, `-32602` explicit-task rejection, and `PROJECT_NOT_FOUND`.
- **V-SEC-01 fix confirmed**: `quality_artifacts.rs`'s `DurableQualityReader` uses `try_lock()` (not blocking `lock()`) for both `read_chunk`/`read_index`, matching the disposition.
- **Tasks enablement is evidence-backed**: `M3-02-budgets.json`'s raw samples match every number quoted in `M3-02.md` and ADR-060 (nextest 1658/1719/1724, coverage 2743/2842/2863, semver 1697/1747/1752 cold p50/p95/p99; 262-byte creation response; 1048-byte job record; 1088ms cancel-to-cleanup; 346ms EOF-to-join) — no fabricated numbers.
- **Execution-mode taxonomy is uniform** across all four tools (verified in nextest.rs, coverage.rs, semver.rs, mutation_test.rs): explicit `task` without declared capability → `-32602`/no data; unqualified `auto` → structured `TASKS_REQUIRED`; mutation's budget floor (clamped to ≥300s) correctly makes it permanently task-only, matching its "task-only" budget entry.
- **Coverage's zero-denominator and path-dedup rules** exist and are tested (`domain/src/coverage.rs`, `execution-adapter/src/coverage_json.rs::deduplicates_a_shared_filename_and_keeps_zero_denominators_absent`).
- Terminal-state publication gating on observed cleanup is enforced in `application/src/job.rs::finalize_joined_cleanup`, consistent with the "cancelled never precedes cleanup" claim.

### Findings reported via ReportFindings
Two confirmed doc-honesty regressions (`docs/compatibility.md` and `README.md` each still contain a stale pre-M3-02 claim that directly contradicts the same document a few lines later, and contradicts `TASKS_ADVERTISEMENT_READY = true`), plus a confirmed test-coverage gap: the Task-creation wiring (`task_materialization_requested`'s message-string match) is exercised end-to-end only for `rust.test.nextest`; `rust.coverage`, `rust.semver.check`, and `rust.mutation.test` — including mutation, which is *always* task-only in practice — have zero live or unit test proving their `execution_mode:"task"` path actually returns a `CreateTaskResult`.

### Missing evidence for closure (not new, already tracked)
ADR-064 and ADR-065 remain formally "Proposed" (per their own Status sections and `M3-matrix.md`); G6/G7/G8/G9 remain Pending. These are outside my contracts angle but block overall M3 Done regardless of this package's verdict.

### Limitations
Read-only pass, no `git diff`/`cargo`/`docker`; I could not independently confirm the 18 M1/M2 snapshot files are byte-identical to `main` beyond the fact that `bootstrap()` asserts them against checked-in snapshot fixtures on every wire-version test run. I did not audit `resources.rs` tests, `tasks.rs` internals, or the ADR-062 SemVer baseline-ingest path beyond what's cited above given the size of the object.