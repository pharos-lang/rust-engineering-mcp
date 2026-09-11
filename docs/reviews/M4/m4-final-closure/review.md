I have completed the read-only review. Below is my finding set.

---

# Independent final M4 closure review — `ai/m4-security` @ frozen source

**Reviewer role:** Principal security/architecture/evidence. **Model:** Claude Opus 5, effort high. **Mode:** read-only (Read/Grep/Glob only; no edits, commands, agents, Docker, git, or network).

## Verdict

**Conditionally favorable on the frozen source; M4 is NOT closed and I do not certify it.**

On the exact bytes listed in `docs/reviews/M4/m4-final-closure/inputs.json` I find **no P0 and no P1**. The seven material deltas are implemented as described and are, with one exception, backed by real receipts bound to the final source hashes. The exception is the **retained P2‑1**: the published scanner and Miri native receipts are still bound to superseded source bytes, and the refreshing run is exactly the pending full gate the owner is executing at dispatch. Prior **P2‑2 is closed in source**.

This review is static. **A passing review is not native test evidence.** Nothing here substitutes for the core/full gate, the client gate, or the native receipts, and the Technical Owner alone updates milestone state.

---

## Prior P2 disposition

### P2‑2 — scanner parser/port absent from the execution fingerprint → **CLOSED (source)**

`crates/execution-adapter/src/security_gateway.rs:807-822` now digests, inside the single `execution_fingerprint` tuple used by all three operations (`Deny`, `UnsafeScan`, `Miri`):

```
808 security_gateway.rs   815 unsafe_scan.rs    819 miri_output.rs
809 security_metadata.rs  816 unsafe_port.rs    820 miri_port.rs
810 security_policy.rs    817 deny_json.rs      821 seccomp-rust-quality.json
811 mutation_gateway.rs   818 security_port.rs
812 rust_applied.rs       813 seccomp-rust.json  814 miri_admission.rs
```

I additionally verified there is **no residual hole** for the guest nextest configuration: `miri_admission.rs:8` embeds `fixtures/m4-runtime-oracles/miri-classification/nextest.toml`, whose bytes would *not* change `digest(include_bytes!("miri_admission.rs"))` — but `security_gateway.rs:703` places that same `CONFIG` into the policy archive, and `security_gateway.rs:792` digests `inputs.policy_archive`. The wrapper/mute flags are therefore bound. Closed.

### P2‑1 — scanner receipt predates gateway bytes → **RETAINED, OPEN (blocks DoD)**

Concrete current mismatch (final bytes taken from `inputs.json` and corroborated by `target/m4-new-native-cuts/receipt.json`, run 04:37–04:40Z on 2026‑09‑08):

| File | Final bytes | `M4-scanner-native.json` | `M4-miri-native.json` |
|---|---|---|---|
| `security_gateway.rs` | `18563e7c…` | `592d1b10…` (:34) | `8e3bf671…` (:58) |
| `rust_applied.rs` | `24349452…` | `fdf5f627…` (:42) | `fdf5f627…` (:66) |
| `miri_output.rs` | `dac2f89d…` | — | `2fea28fa…` (:42) |
| `miri_native.rs` | `a99cef24…` | — | `1fa720a4…` (:54) |

`rust_applied.rs` and `security_gateway.rs` are both inside the execution fingerprint, so neither native receipt attests the final fingerprint. Relatedly, `docs/validation/M4/miri-native.json:22` records `classification_cases: 12`, while `crates/execution-adapter/src/miri_native.rs:33-47` now enumerates **13** (`optimized-panic` added); `docs/adr/ADR-072-miri-classification-integrity.md:5-6` still states "12 casos" citing that receipt.

The closure path is credible and already wired, which is why I classify this as a pending gate rather than a code defect: `scripts/test-m4-runtime.py:70` selects `unsafe_native::m4_scanner_native_oracles_…`, both `miri_native::…` selections, `security_native::…`, the deny/graph/adversarial cuts and the three MCP cuts; lines 99‑100 and 170‑173 hash **every** `crates/**/*.rs` before and after and fail on drift; lines 61/65 pin the image from `docs/validation/M4/runtime-image.json` (`25ed…`); `scripts/gate.py:182` runs it in the full gate.

**Required correction within P2‑1 (documentation asymmetry):** `docs/validation/M4/matrix.md:48` and `docs/implementation-status.md:298-299` correctly retain the Miri rerun requirement, but `M4-matrix.md:47` and `implementation-status.md:275` present the scanner suite as passed **with no staleness caveat**. The two rows must be symmetric before Done.

Other prior dispositions (P3‑1…P3‑10 in `docs/reviews/M4/m4-security-confirmation/disposition.md`, and the bounded composition/contract findings in `docs/reviews/M4/m4-composition-confirmation/review.md` and `docs/reviews/M4/m4-traceability/disposition.md`) I accept as already reviewed and unchanged in this snapshot. I did not re-litigate them.

---

## Findings

**P0 — none. P1 — none.**

### P2

**P2‑1 (retained, above).** Scanner/Miri native receipts not bound to final bytes; matrix/status disclosure asymmetric for the scanner row. Blocks DoD until the full gate publishes `target/m4-runtime/receipt.json` and the published receipts/ADR‑072 counts are refreshed.

*No new P2 identified.*

### P3

**P3‑A — the fingerprint file set has no regression oracle.** `security_gateway.rs:807-822` is an unguarded literal list; the module's tests (`:1379`, `:1405`, `:1438`, `:1512`, `:1548`, `:1573`, `:1606`) pin arguments, mounts, env and phase mapping but none pins the digested source set. A future parser/port module could be added and silently omitted, exactly reproducing P2‑2. Cheap fix: a test asserting the tuple arity/paths.

**P3‑B — benchmark provenance is disclosed but not characterizable, and the Codex sync authorization is untraced.** `docs/validation/M4/budgets.json:6` records `binary_sha256: d27cbc59…` and `:7` scopes it ("frozen binary; synchronous protocol routing and final source-bound gate require separate receipts"); `M4-matrix.md:59` and `implementation-status.md:289` say plainly it credits *a prior binary, not the current final bytes*. **The mismatch is disclosed, not silently treated as final-binary latency — this satisfies item 7.** Residual: the receipt does not name the source snapshot that produced `d27cbc59…`, so the routing/failure-parser delta cannot be bounded from evidence. Separately, `scripts/test-m4-clients.py:409-410` gates the synchronous run on the env var `RUST_MCP_M4_CODEX_SYNC_QUALIFIED=1`, and the passing receipt `docs/validation/M4/clients.json` contains **no** record of the budgets receipt that justifies the 60 s ceiling (the field exists only in the preflight schema, `test-m4-clients.py:117-122`).

**P3‑C — closure documents lag the newest receipts (understatement, not overclaim).** `M4-matrix.md:3` is dated 2026‑09‑07; `:60`, `:63` and `:94` still list clients and tamper as pending, and `implementation-status.md:294,300` say the same — yet `docs/validation/M4/clients.json:7039` and `docs/validation/M4/tampered-plugin.json:1268` are both `"passed"` (2026‑09‑08). Plus the ADR‑072 case count noted under P2‑1. These must be brought current at closure.

**P3‑D — `assert_no_credentials` is filename-shaped only.** `scripts/test-m3-clients.py:539-546` rejects `CREDENTIAL_NAMES` and `*.sqlite*` under the attempt directory. It is a staging guard, not content secret detection. The real control is architectural and holds: `test-m4-clients.py:337-340` symlinks `auth.json` into a `/private/tmp` `CODEX_HOME`, never copying it; `:394` records `auth_copy: false`; `:140-148` rejects credential-shaped protocol metadata. Keep the limit explicit.

**P3‑E — JUnit-present classification ignores process-level streams.** `miri_output.rs:1006-1011` takes `_stdout`/`_stderr`; when JUnit exists, classification rests on JUnit structure, runner exit, global exit and per-testcase `system-err`. This is ADR‑072:40‑41 by design and is safe (guest streams are muted by `-Zmiri-mute-stdout-stderr`, and `:1038-1050` cross-checks the expected global exit and empty-suite). Recorded as a bounded residual, **not** a request to redesign.

---

## Verification of the seven material deltas

**1. Fingerprints / image binding — verified.** Above. Image pin `sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635` is consistent across `lib.rs:36`, `miri_native.rs:16,192`, `security_native.rs:48,61`, `unsafe_native.rs:12`, `rust_gateway.rs:936`, `docs/validation/M4/runtime-image.json:2`, and every M4 receipt I read.

**2. `miri_output.rs` narrow warning permit — verified sound.** `:861-875` admits **exactly** the pinned nightly text with `level=="warning"`, `code==null`, `spans==[]`, `children==[]`, and sets only `blocks_test_failure=false`. Because `JsonEvidence::merge` is `|=` (`:751-760`) and every analyzed line starts `blocks_test_failure: true` (`:826-830`), **any** additional JSON line, malformed line, oversize line, non-`diagnostic` payload, missing `level`/`message`, or (in a failed testcase) any non-JSON line (`:916-917`) forces the exit‑101 branch (`:997-1001`) shut → `Unclassified`, `complete=false`. UB/unsupported/post-mono remain gated on `level=="error"` at exit 1 with `known_categories==1` (`:980-996`); a warning bearing UB-shaped prose sets `unexpected` (`:876-881`). **No path makes unknown JSON produce UB or clean.** Clean is unreachable via this permit: passing cases require empty `system-err` (`:1018-1020`) and `clean` additionally requires `complete && exit==0 && passed==tests && failed==0 && skipped==0 && findings_omitted==0` (`:1052-1058`).
- Golden: `fixtures/m4-runtime-oracles/miri-classification/optimized-panic/{Cargo.toml:5-6 opt-level=1, src/lib.rs:2, junit.xml}` — a real capture (UUID/timestamp 2026‑09‑08T04:34:44Z) whose source forges `Undefined Behavior:` on stdout and then panics.
- Mutation oracles: `miri_output.rs:1206-1239` — five independent mutations (message, level, code, children, spans), each asserting `test_failures==0`, `undefined_behavior==0`, `unclassified==1`, `!complete`.
- Native classification: `miri_native.rs:105-108` asserts `optimized-panic` → `test_failures==1`, `undefined_behavior==0`, and `:91-95` `clean==false`.
- MCP canary proof: `crates/mcp-server/tests/inspection_runtime/security.rs:751-768` — `rust.miri` returns `status=="failed"`, `undefined_behavior==0`, `test_failures==1`, `compile_failures==0`, finding `test_name=="hostile_output"`, with the canary absent from the result and from **each** artifact (`:777-793`, `cacheScope=="private"`).
- Scope: `security_gateway.rs:111-127` runs `cargo miri nextest run --workspace --tests …` (lib/bin unit + integration; no doctests/examples/benches), corroborated by the `binary-only` oracle (`miri_native.rs:49-53`) and ADR‑072:94‑98. Bounded-interpretation and no-absence-of-UB claims are stated at ADR‑072:113‑115 and `docs/tools.md:1131-1134`.

**3. Tampered-plugin probe — verified corrected.** `scripts/test-m4-tampered-plugin.py:768` uses `--no-pause`; `:52,244-248` parses the ID with a full-line anchored regex requiring exactly one unique identity (the exact prior defect, regression-tested at `test-m4-safety-harnesses.py:169-178` with the literal deprecation prose). Discovery uses `image ls --all --no-trunc --quiet --filter label=<owner>=<nonce>` (`:452-465`), asserted in `test-m4-safety-harnesses.py:213-217`. Ownership is enforced before any removal (`:468-483`, `:598-604`) and `identity == APPROVED_IMAGE` is explicitly refused (`:600`) — **no arbitrary image deletion**. Output is bounded by `RLIMIT_FSIZE` + explicit checks (`:112-129`, `:205-207`) with unit coverage (`:60-64` of the harness tests) and a bounded single-member TAR copy (`:333-359`, tested at `:118-167`). Cleanup fail-closes: an unresolved commit cannot claim `verified` (`:515-522`, `:605-606`, `:624-641`; tested at `:180-217`).
Admission: the altered image is rejected at CLI parse — `crates/mcp-server/src/host_config.rs:146-151` accepts only the three pinned digests, so `main.rs:180` returns exit 2 with `USAGE_ERROR` **before any Docker or guest code runs**; the script asserts empty stdout, exact stderr, no state directory, unchanged product Docker inventory and unchanged project fixture (`:809-816`), with the approved-image EOF control (exit 0) as the positive discriminant (`:820-830`). Receipt `docs/validation/M4/tampered-plugin.json` is `passed`, `script_sha256: 77d7a18f…` matching `inputs.json`, `inputs_unchanged: true`, `cleanup.verified: true`. Failure/repair evidence preserved and honestly labelled at `docs/validation/M4/history/hardening-attempts/README.md:5-7` and `.../attempt-2/cleanup-repair.json` (`original_cleanup_claim_valid: false`).

**4. Clients — verified, and bound to the final source.** `test-m4-clients.py:31` uses the **release** binary; `:426-429` embeds a full `gate.source_inventory`, re-checked at `:443-446`. `docs/validation/M4/clients.json` (`status: passed`, attempt‑2) contains `security_gateway.rs 18563e7c…` (:1319), `miri_output.rs dac2f89d…` (:1158), `deny_json.rs 2c7a5ea1…` (:1116), `unsafe_scan.rs e1703314…` (:1466) — **exactly the final bytes**. Inspector 2.5.0 (`bundle_sha256`, `m4-inspector-session.mjs:57,79-125`): ordered 27‑tool discovery, 5 positives with Resource reads, 5 negatives (`isError` + blocked/unavailable, or masked `-32602`), and a real Tasks cancel on `rust.miri` ending `cancelled`. Codex 0.153.0 stock: synchronous 60 s conversion for all five tools each requiring `passed` (`:354-359`), negative preserved (`:360-362`), resources validated (`:363-365`), then a **model-directed** turn requiring `{project.open} ∪ M4_TOOLS ⊆ passed` (`:389-390`) — receipt `:6920-6928` shows all six. `tasks_declared: false`, `task_cancel: "not supported by stock client"` — **no Tasks claim for stock Codex**. Credentials stay outside the repo (P3‑D). Cleanup observed: `:398-399` requires `cleanup_verified`; `private_fixture_removed: true`. Attempt‑1 failure preserved (`rpc_thread/start_error_-32603`, `status: failed`).

**5. Privacy — verified with both controls.** `crates/execution-adapter/tests/m4_privacy_runtime.rs`: positive authorized-source canary asserted **present** in coverage HTML (`:114`) and in an actual generated mutation diff via a real USTAR walk of `./diff/` regular members (`:22-55`, `:139`); negative host-only canary (random nonce, provably absent from captured source at `:90`) asserted absent from HTML/JSON/LCOV/stdout/stderr (`:115-126`) and from the mutation bundle/streams (`:142-151`); host file byte-identical afterwards (`:152`); owned containers/volumes empty (`:154-176`). Receipt `target/m4-privacy-runtime.json:11` records `"universal_source_secret_redaction": false`. `SECURITY.md:355-360` documents precisely this distinction. MCP side reads **each** artifact (security.rs:777-793) and distinguishes ordinary panic from UB despite guest prose (P2 item 2 above).

**6. Miri lifecycle and withdrawal — verified.** `security.rs:620-698`: for `cancel`/`eof`/`revocation` the Miri container is first *observed running* (`:591-616`, matching `/opt/rust-nightly-2026-09-07/bin/cargo miri nextest run`); revocation yields `task unavailable` `-32602` (`:662-663`); cleanup is awaited **before** EOF for cancel/revocation (`:676-684`) and re-asserted after `finish()` for all three (`:686`). Deny withdrawal (`:202-232`): restart on `APPROVED_RUST_IMAGE` (M3) → `unavailable` / `TOOL_NOT_INSTALLED`, the **same** audit bytes still readable under a new `ProjectRef` of the same owner, then revocation makes it error. Enforcement is structural: `miri_port.rs:18-20` and `unsafe_port.rs:21-23` return `Unavailable` unless `gateway.image_id() == APPROVED_M4_IMAGE`. `test-m4-runtime.py:144` records the M3 rollback digest explicitly for the one selector that needs it; all 19 selectors run under `RUST_MCP_TEST_IMAGE=25ed…` (`:65`), and the legacy M3 suite reads that env (`rust_calibration.rs:102`, `coverage_runtime.rs:36-41` which asserts the image is one of the two) — the legacy suite therefore stays a separate, explicitly imaged cut.

**7. Receipts / inventory / benchmark / snapshots.** Inventory `docs/validation/M4/runtime-inventory.json`: `passed`, all **six** pinned binaries matched (`:354-385`, pins at `test-m4-inventory.py:34-41`), full Miri sysroot verified as a tree digest over 82 files / 114 335 161 bytes (`:406-408` vs pin `:28`), `guest_code_executed: false`, `network_used: false`, `never_started: true`, image inspect identical before/after, `script_sha256 9a5a3fff…` matching `inputs.json`. Benchmark: 300 observations (10 groups × 30) in `M4-budgets.json:10-111`, max 17 044 ms under a 60 s ceiling, with the pre-final-binary mismatch **disclosed** (see P3‑B). Snapshots: 28 files under `crates/mcp-server/tests/snapshots/`, i.e. the 23 pre-existing plus `deny/unsafe-scan/supply-chain/quality-v2/miri` (`protocol.rs:402-428`); "unchanged" is a structural confirmation only — I cannot run `git diff`.

---

## Mandatory evidence still missing (pending, not newly discovered)

1. Final source-bound **full** gate (`scripts/gate.py full`) producing `target/m4-runtime/receipt.json` with all 19 selectors on `25ed…` — this is the artifact that discharges P2‑1 and re-binds P2‑2.
2. Final source-bound **core** gate.
3. Refreshed/superseded-marked `docs/validation/M4/scanner-native.json` and `M4-miri-native.json`, ADR‑072 case count, and symmetric matrix/status wording.
4. A separate evidence-only confirmation after the gates (which the owner has already scheduled).

## Reviewed files and evidence

Source: `security_gateway.rs`, `miri_output.rs`, `miri_native.rs`, `miri_admission.rs`, `miri_port.rs`, `unsafe_port.rs`, `security_inventory.rs`, `lib.rs`, `host_config.rs`, `mcp-server/src/main.rs`, `mcp-server/tests/inspection_runtime/security.rs`, `execution-adapter/tests/m4_privacy_runtime.rs`, `rust_calibration.rs`, `coverage_runtime.rs`, `mcp-server/tests/protocol.rs`, `stdio/{deny,unsafe_scan,supply_chain,quality_v2,miri}.rs` (advertisement switches).
Harnesses: `scripts/{gate.py, test-m4-runtime.py, test-m4-tampered-plugin.py, test-m4-inventory.py, test-m4-safety-harnesses.py, test-m4-clients.py, m4-inspector-session.mjs}`, `test-m3-clients.py:532-551`.
Fixtures: `fixtures/m4-runtime-oracles/miri-classification/{nextest.toml, optimized-panic/*}`.
Evidence: `target/m4-new-native-cuts/receipt.json`, `target/m4-privacy-runtime.json`, `docs/validation/{M4-clients,M4-tampered-plugin,M4-runtime-inventory,M4-budgets,M4-runtime-image,M4-scanner-native,M4-miri-native,M4-matrix.md}`, `docs/validation/M4/history/hardening-attempts/**`, `docs/validation/M4/clients/attempt-1/receipt.json`.
Docs/ADR: `ADR-072`, `docs/tools.md:1047-1145`, `SECURITY.md`, `docs/implementation-status.md:263-307`, `docs/ci.md`, `docs/roadmap/m4-security.md`, `docs/prompts/history/implement-m4.md`, and the three prior review dispositions.

## Remaining limitations of this review

Static only — I executed nothing and computed no hashes; every hash comparison is between values *declared* in `inputs.json` and values *recorded* in receipts. I cannot confirm that the on-disk bytes equal the declared hashes, that the 23 legacy snapshots are byte-identical to `main`, or that any receipt reflects a run that actually occurred. Claims about container behaviour, cleanup and latency rest on reading test and harness code. The disposition of every finding, and any accreditation of M4, belongs to the Technical Owner.
