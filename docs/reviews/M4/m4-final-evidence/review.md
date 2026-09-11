I have completed the read-only evidence review. Below is my finding set.

---

# Independent final M4 evidence confirmation — `ai/m4-security` @ `c66a3704`

**Role:** independent final evidence reviewer. **Model:** Claude Opus 5, effort high. **Mode:** read-only (Read/Grep/Glob only; no commands, agents, tests, edits, Docker, git or network).

## Verdict

**ACCEPT local M4 closure.** No P0, no P1, no new P2. The retained prior **P2‑1 (stale scanner/Miri native receipts) is CLOSED by fresh receipts**, and the closure is not an owner assertion: it is corroborated by an unbroken hash-and-timing chain from the final gate receipt down to raw per-case receipts.

This acceptance is scoped exactly as the package scopes itself: local qualification on macOS 26.6.2 ARM64 with the pinned Linux ARM64 runtime. It credits **no** remote CI, no Sonar run, no Linux/x86_64 host, no commit/PR/tag/release, and no M5. The status change to Done remains the Technical Owner's act.

---

## Disposition of the retained prior P2

### P2‑1 — scanner/Miri native receipts predate the final gateway bytes → **CLOSED (evidence)**

The four files the prior review named as mismatched now carry the **final** bytes in both published native receipts, identical to the core/full/clients inventories:

| File | Final (gate inventory) | `M4-scanner-native.json` | `M4-miri-native.json` |
|---|---|---|---|
| `security_gateway.rs` | `18563e7c…` (full-gate.json:3006) | `:675` | `:695` |
| `rust_applied.rs` | `243494 52…` (:2943) | `:659` | `:679` |
| `miri_output.rs` | `dac2f89d…` (:2845) | `:603` | `:623` |
| `miri_native.rs` | `a99cef24…` (:2838) | `:599` | `:619` |

Decisive corroboration that these receipts describe the *final gate's own* executions rather than a side run:

- `M4-scanner-native.json:23` `seconds: 45.672` **equals** `M4-runtime.json:301`, the `unsafe_native::…` selection.
- `M4-miri-native.json:42` `seconds: 202.007` **equals** `155.687 + 46.32` (`M4-runtime.json:327,353`), the two `miri_native::…` selections.
- `M4-runtime.json` ran `06:12:56.958 → 06:24:49.564`, nested exactly inside the full gate's `m4-runtime` step `06:12:56.903 → 06:24:49.628` (`M4-full-gate.json:1429,1432`).
- Both native receipts pin `runtime_receipt_sha256 b9eb1d1d…` (scanner `:1793`, miri `:1811`), matching `inputs.json` and the evidence index.

Case coverage and cleanup are present in the **raw** receipts, not only in summaries: scanner 7 cases (`final-report.json:4,78,271,345,419,493,601`) with `cleanup.all_absent: true` (:641); Miri 13 classifications (`final/receipt.json:4…412`, including the new `optimized-panic` at `:4` with `test_failures 1 / undefined_behavior 0`) plus 7 admission/lifecycle cases each `cleanup_verified: true` (`adversarial-receipt.json`), with `miri_running_observed: true` for `timeout` and `cancel`. Receipt hashes match `inputs.json` (`42c56f7d…`, `f454be85…`, `9d1be961…`).

Documentation asymmetry required by the prior review is resolved: `M4-matrix.md:46-47` and `implementation-status.md:280` now state both rows as bound to current sources, and `ADR-072:5-6` reads "13 casos … y 7 de admisión/ciclo de vida (202.007 s)" — matching the receipt exactly. Superseded receipts are preserved (`docs/validation/M4/history/hardening-attempts/native-before-final/`).

### P2‑2 — parser/port modules absent from the execution fingerprint → **remains CLOSED**, now receipt-bound

Not re-litigated in source (prior code review's scope). The relevant point here is that the refreshed receipts now attest the post-fix bytes, and `M4-runtime.json`'s `configuration_inputs` carry the guest policy inputs at final values — `seccomp-rust-quality.json c288305c…` (:1933), `seccomp-rust.json f9d31acb…` (:1937), `miri-classification/nextest.toml 621739d9…` (:2073) — each identical to `M4-full-gate.json:2978,2985,6051`. "native19 sources/config current" is verified.

---

## Verification of the specific claims I was asked to check

**Same 987 inputs across core / full / clients — verified to the limit of static review.** `M4-core-gate.json`, `M4-full-gate.json` and `M4-clients.json` (`candidate.sources`) each contain exactly **987** inventory entries with the same schema. First entries (`.github/CODEOWNERS b4f158e9…`), last entries (`vendor/lancedb/tests/object_store_test.rs bde69a4b…`) and every M4-critical file I sampled are identical across all three. Core `:8202` and full `:8617` both record `source_inputs_unchanged: true`; `M4-client-execution.json:21,127` records the recheck and the three-way equality. Limitation: I compared counts, endpoints and ~15 sampled entries, not all 987 elementwise, and I computed no hashes.

**Full 33 resumption — no skip, no oracle weakening, no source weakening.** The published receipt lists 33 steps in exactly `gate.py`'s full order (`M4-full-gate.json:9…1668` vs `gate.py:150-189`), all `passed`/`exit_code 0`, none `running` or `failed`. The driver:
- reloads the **real** `scripts/gate.py` and calls the **original** `gate.run_step` (`resume-driver.py:4,22`); `gate_script_sha256 6a487182…` in the receipt (`:8609`) matches `inputs.json`;
- asserts the prior receipt is `full`/`failed` with 28 steps whose first 27 all passed, and that indices 25/26 are `m4-runtime`/`audit-data` (`:6-9`) — so retention stops exactly at the real boundary;
- re-runs the six remaining commands **byte-identical** to `gate.py:184-189`, with the same `require_test_groups=False` default — no oracle relaxed;
- asserts the live inventory equals the prior receipt's before running (`:14`) and again after (`:24`), reproducing `gate.py:190-192`;
- asserts `cargo`/`rustc --version` equal the prior receipt's exact strings (`:15`) — both receipts carry `cargo 1.98.1 (797e8a9bc …)` / `rustc 1.98.1 (48a229cea …)`, matching core.

The original failed receipt is preserved and its cause is honestly characterised. `full-attempt-2/receipt.json:1470-1517` shows `semantic` failed with one failing group; the preserved log shows the mechanism verbatim: `real_offline_e5_lance_sqlite_roundtrip … Error: Os { code: 2, kind: NotFound }` → `semantic failed (1)`. The resumed run's same step shows that group flipping to `passed: 1` with real model output. **The five recovered assets are not substitutes**: `M4-e5-local-recovery.json` records `model.onnx ca456c06…` and `tokenizer.json 0b44a9d7…`, which are the values pinned in `crates/semantic-adapter/src/model.rs:9,14` — i.e. the product's own pins, `network_used:false`, `downloaded:false`, total 487 352 503 bytes as stated in the handoff. No claim that the first monolithic invocation passed appears anywhere; `M4-handoff.md:144-145` states the opposite explicitly.

**Runtime/image identity.** All 19 selections in `M4-runtime.json` run on `sha256:25ed3626…91635`; exactly one, the deny rollback selector (`:359`), carries `rollback_image_id sha256:384a1742…` (M3). The legacy regression suites keep their own image: `M4-m3-runtime.json:5` and `M4-rust-security.json:5` are both `384a1742…`. `M4-tampered-plugin.json` is `passed` with `cleanup.verified true`, `inputs_unchanged true`, `containers_started false`, `image_published_or_tagged false`.

**Delta 1 — protocol.rs stdout observer: diagnostics only.** `protocol.rs:1106-1132` accepts **only** `Err(Disconnected)`; `Timeout`, `Ok(Ok(frame))` and `Ok(Err(io))` all return errors, and the subsequent `status.code() == Some(1)` plus non-empty stderr assertions are unchanged. `TIMEOUT` is still `Duration::from_secs(10)` (`:11`). The repetition receipts bind the **final** test bytes (`source_sha256 c5f81a1c…` = `inputs.json`): 30/30 isolated and 5/5 full protocol suites, all exit 0. The disposition states plainly that the original failure's mechanism cannot be established because the old assertion discarded the variant — no causal fix is claimed.

**Delta 2 — corpus SHA.** The only relevant entry, `fixtures/security/rust-containment/checks.rs a6e9cc11…` (`corpus-sha256.json:27`), matches `inputs.json` exactly. The five added lines (`checks.rs:324-328`) *strengthen* the fixture: connecting to `169.254.169.254:80` must fail with `PermissionDenied`. This is an added containment assertion, not a weakened oracle, and it is exercised on both images (`rust-security` 20/20 on `384a1742…`; the `rust_calibration` containment selections on `25ed…`).

**Full attempt 1 legacy qualifier cleanup failure.** `isolated-large-binary-10.json` contains 10 iterations, each `passed` with both `missing_runtime` and `repair` phases showing `threads_joined true`, `forced false`, `remaining_pids []`, `failure null`. The disposition retains "the exact cause of the single failure is unestablished" and forbids counting it as passed. Correct posture; no erasure claimed.

**P3‑B supplement — benchmark provenance.** Now bounded rather than merely disclosed. `M4-budgets/m4-budgets-inputs.json` carries **341** source hashes paired with `binary_sha256 d27cbc59…` (:1369); `M4-benchmark-source-continuity.json` reports `unchanged_source_files: 309` plus **32** named changed files with both hashes (309+32 = 341 ✓). Its benchmark-side hashes for `miri_output.rs 2fea28fa…`, `rust_applied.rs fdf5f627…` and `miri_native.rs 1fa720a4…` are precisely the values the prior review found in the then-stale native receipts — independent internal corroboration that the frozen binary predates the final bytes. The changed set correctly includes the routing surface (`stdio/{deny,miri,quality_v2,supply_chain,unsafe_scan}.rs`, `main.rs`, `security_gateway.rs`), which is exactly why the benchmark must stay historical. Every public statement says so (`M4-matrix.md:58`, `M4-handoff.md:81`, `implementation-status.md:283`, `ADR-067:278-284`). **No document claims 300 samples on the final binary**, and I do not demand one. The sync authorization is now traced: `M4-client-execution.json:121-126` links `RUST_MCP_M4_CODEX_SYNC_QUALIFIED=1` to `M4-budgets.json` with sha256 `84fe208b…`, with an explicit "no claim final binary is benchmark binary".

**Clients attempt 4.** `M4-clients.json:7039` `passed`, bound to `f4e2c6d1…` and `25ed…`; Inspector 2.5.0 with 27 tools, 5 positives, 5 resource reads, Tasks cancel; Codex 0.153.0 stock with 5 synchronous positives, negative, resources and a model-directed turn whose 6 tools are all in `model_turn_passed_tools` (`:6921-6928`); `tasks_declared: false` and `task_cancel: "not supported by stock client"` — no Tasks overclaim. `auth_copy: false`, `private_fixture_removed: true`. Cleanup fail-closes in the harness itself (`test-m4-clients.py:398-399` raises "Codex cleanup unverified"), and the non-retention of the raw cleanup object is disclosed at `M4-client-execution.json:119`. Auth stays outside the repository; the filename guard is explicitly not advertised as a secret scanner (`M4-evidence-index.json:29`).

**Public docs and matrix.** They qualify the executions and stop short of Done: `implementation-status.md:150` still "In progress", `:268` "Calificación completa; confirmación independiente final pendiente"; `M4-matrix.md:50,63,64` keep G8/G9 and M4‑06 pending; `README.md:21-22` states closure awaits final evidence confirmation; `CHANGELOG.md:13-14,24-25` claims no release/tag/PR and defers Done. ADR‑072's JUnit boundary (`:113-115` bounded interpretation), scanner incompleteness (`tools.md:1076` — no macro expansion, no `cfg`, no generated code) and the HTML/diff limit (`README.md:74-77`, `security-model.md:545`, `M4-privacy-runtime.json:11 universal_source_secret_redaction: false`) all remain explicit.

---

## Findings

**P0 — none. P1 — none. P2 — none (prior P2‑1 closed above; P2‑2 stays closed).**

### P3 (all minor; none blocks local closure)

**P3‑1 — the resume driver reimplements `gate.py`'s preflights instead of invoking them.** `resume-driver.py:10-15` hardcodes `RUST_MCP_TEST_SOCKET`, `RUST_MCP_E5_DIR` and `ORT_LIB_LOCATION` and skips `gate.py:132-147` (darwin/arm64 check, `shutil.which` for rustup/cargo-audit/cargo-deny, version-prefix assertions) and the `if not __debug__` guard at `:200`. Materially mitigated — toolchain identity is asserted equal to the prior receipt's exact strings, none of the six steps uses audit/deny, and the E5 path change is the documented point of the recovery — but a future resumption should call the preflight block rather than restate it.

**P3‑2 — the published full receipt presents one timeline for two segments.** `M4-full-gate.json:6` `started_at 05:33:44` is the original attempt's start, and no per-step field marks which 27 rows were retained; only the `resumption` block (`:8602-8616`) and the prose in the handoff/CHANGELOG disambiguate. A `retained: true` flag per step, or a `segments` array, would make the receipt self-describing. Disclosure elsewhere is honest, so this is presentation, not overclaim.

**P3‑3 — the frozen package omits the two gate logs.** `docs/validation/M4-full-gate.log` and `M4-full-gate-resume.log/.txt` exist on disk but are not in `inputs.json`, although they are the strongest anti-skip artefact. I read `M4-full-gate-resume.txt` and `full-attempt-2/gate.txt` **outside** the frozen package (disclosed below) precisely because the package alone could not settle "no skip" and "cause of the E5 failure". Include them next time.

**P3‑4 — "prior snapshots unchanged" is not checkable from the package.** `M4-client-execution.json:23-116` lists 23 snapshot hashes but the package carries no `main` baseline to compare against, so `prior_snapshots_unchanged: 23` (`M4-evidence-index.json:23`) rests on the observer's own comparison.

**P3‑5 — the E5 recovery receipt names its pin source but not the pin values.** `M4-e5-local-recovery.json:36` cites `crates/semantic-adapter/src/model.rs` without recording the expected hashes or an explicit match assertion. I corroborated the match externally (`model.rs:9,14`); the receipt should carry `expected_sha256` alongside `sha256` so it stands alone.

**P3‑6 — `M4-runtime.json` records the source/config inventory but no post-run "unchanged" boolean.** The before/after drift check lives in `scripts/test-m4-runtime.py`, which is not in the package; the receipt itself only implies it via `exit_code 0`.

**P3‑7 — `source_inventory_sha256 515dac…` has no stated canonicalisation.** `M4-evidence-index.json:6` gives a single digest without specifying the serialisation over which it is computed, so it is not reproducible from the package; the three embedded 987-entry inventories remain the real evidence.

**P3‑8 — carried, accepted limits, all correctly disclosed and none re-opened by me:** raw Codex cleanup object not retained; `assert_no_credentials` is filename-shaped, not a content scanner; ADR‑072's JUnit-present classification ignores outer process streams; the scanner is syntactic and incomplete; no universal redaction of authorized source in coverage HTML / mutation diffs. Prior **P3‑A** (no regression oracle pinning the fingerprint file set) remains open as Technical Owner backlog by the owner's own disposition; I agree that a test which merely mirrors the literal list would not be an improvement over the native source-bound evidence.

*Note for readers of `M4-output-canaries.json`: the `status` values (`failed` for `rust.miri`, `blocked` for `rust.quality.gate.v2`) are tool-level outcomes of hostile fixtures, not gate failures; `canary_absent` is `true` for all five.*

---

## Evidence I inspected

`inputs.json` and, under `inputs/`: both closure documents; `M4-evidence-index.json`; `M4-matrix.md`; `M4-handoff.md`; `M4-hardening-map.md`; `M4-core-gate.json`; `M4-full-gate.json`; `M4-full-gate-resume-driver.py`; `scripts/gate.py`; `M4-hardening-attempts/full-attempt-2/receipt.json`; `M4-runtime.json`; `M4-scanner-native.json` + `M4-scanner-native/final-report.json`; `M4-miri-native.json` + `final/receipt.json` + `final/adversarial-receipt.json`; `M4-clients.json`; `M4-client-execution.json`; `scripts/test-m4-clients.py`; `M4-tampered-plugin.json`; `M4-privacy-runtime.json`; `M4-output-canaries.json`; `M4-miri-task-lifecycle.json`; `M4-e5-local-recovery.json`; `M4-budgets.json` + `M4-budgets/m4-budgets-inputs.json`; `M4-benchmark-source-continuity.json`; `M4-rust-security.json`; `M4-m2-runtime.json`; `M4-m3-runtime.json`; `M4-runtime-inventory.json`; `crates/mcp-server/tests/protocol.rs`; `fixtures/corpus-sha256.json`; `fixtures/security/rust-containment/checks.rs`; the stdout-diagnostic and full-attempt-1 dispositions with their receipts; ADR‑066/067/068/069/070/071/072; `README.md`, `CHANGELOG.md`, `docs/{tools,compatibility,client-configuration,architecture,security-model,ci,implementation-status}.md`, `SECURITY.md`.

**Reads outside the frozen package (disclosed):** `docs/validation/M4/full-gate-resume.txt` and `M4-hardening-attempts/full-attempt-2/gate.txt` (to test "no skip" and the E5 failure mechanism against raw output rather than owner prose); a grep of `crates/semantic-adapter/src/model.rs` (to test the asset pins); and directory listings of `docs/validation/M4/history/hardening-attempts/**` and `M4-full-gate*` (to confirm the preserved failure receipts, `native-before-final/` and the separate segment logs exist). Nothing was written or executed.

## Limits of this confirmation

Static only. Every hash comparison is between values *declared* in `inputs.json` and values *recorded* in receipts; I computed no digests and ran nothing, so I cannot confirm that on-disk bytes equal the declared hashes, that the 23 legacy snapshots are byte-identical to `main`, or that any receipt reflects a run that actually occurred — only that the receipts are mutually consistent in hash, count and wall-clock nesting to a degree that would be difficult to fabricate accidentally. Container behaviour, cleanup and latency rest on reading harness code and receipts. This confirmation covers **local** M4 closure only: remote CI, Sonar, Linux and x86_64 remain unqualified and I do not ask for them absent publication authorization. Accreditation and the status change to Done are the Technical Owner's.
