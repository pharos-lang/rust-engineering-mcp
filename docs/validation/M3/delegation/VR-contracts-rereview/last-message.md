## Verdict: **Revise**

M3 cannot close yet — not because the delta's engineering is unsound (it isn't), but because the milestone's own closure records now contradict each other about ADR-064/065 acceptance, and because G8 (this re-review) is exactly the missing piece the records already say is missing.

### 1. Disposition of prior findings

| ID | Status | Evidence verified |
|---|---|---|
| VF-01 (public docs said Tasks unadvertised) | **Resolved** | `SECURITY.md:326-328`, `README.md:398,414,439,442`, `CHANGELOG.md:10,24-25`, `docs/architecture.md:300,336`, `docs/implementation-status.md:23,43,121,254` all now say Tasks is implemented, qualified and advertised after G4, gated on peer declaration. No trace of the old "no se anuncia"/"anuncio OFF" language remains. |
| VF-02 (55/55 vs 59/59 count mismatch) | **Resolved, superseded consistently** | Gate grew to 62/62 (nextest 19, Tasks 7, coverage 8, SemVer 18, mutation 10) after G1 closure; `M3-matrix.md:8,15,27`, `M3-07.md:23`, `CHANGELOG.md:10`, `implementation-status.md:145,253` all cite 62/62 with the identical breakdown. Historical `55/55` in `ADR-065:137` is explicitly labeled "(W3, …)" as a dated citation, not a live claim. |
| VF-03 (stale ADR index) | **Resolved** | `docs/adr/README.md:75-80` now reads "Accepted; implemented and M3-02 qualified" etc. for 060-063, and "Accepted 2026-09-06; qualified by the M3 runtime and Rust security receipts" for 064/065. |
| VF-04 (nextest samples resumed, undisclosed) | **Resolved** | `docs/validation/M3-02.md:47`: "nextest samples were carried forward from the current attempt log rather than [freshly measured]". |
| VF-05 (Inspector Cancel/Resource cells hid CLI failures) | **Resolved** | `M3-02.md:73-82` — table cells now carry `[^inspector-cancel]`/`[^inspector-resource]` footnotes stating the one-shot CLI exits non-zero and qualification is through the persistent session. |
| VF-06 (undocumented transient `unavailable`) | **Resolved** | `docs/tools.md:1030-1032`: "An `unavailable` member observed during polling may be a transient projection of registry/store contention…polling again…can make the live member visible." |
| VF-07 (`test-hooks` override only forces advertisement on) | **Still open, accepted as non-blocking gap** | `M3-07.md:270-272` explicitly records it as an accepted, registered gap; no code change. Matches my original P3 disposition. |
| VF-08 (`LiveJobAuthority::revalidate` contention→authorized, invariant unstated) | **Still open, not addressed** | No mention in any W6/D03/W5 report; code at `tasks.rs:528-534` presumably unchanged. Correctly not merge-blocking per my original disposition. |
| VF-09 (no completed gate receipts) | **Resolved** | `M3-core-gate.json` — all steps `"status":"passed"`; `M3-full-gate.json` — 25 steps, `audit-data` at line 1186-1204 shows `"status":"passed","exit_code":0`; `"source_inputs_unchanged": true` at line 7042. |
| VF-CONTRACTS gap (only nextest had a live `CreateTaskResult` proof) | **Resolved, real oracle** | `tasks.rs:408-421,423-514` — new `tasks_coverage_/tasks_semver_/tasks_mutation_materializes_a_create_task_result_*` tests, each asserting the full envelope (`assert_created_task_envelope`, `tasks.rs:53-76`: pinned `taskId`, `ttlMs:7200000`, `pollIntervalMs:1000`, `createdAt==lastUpdatedAt`, non-empty `statusMessage`, absent `content`/`structuredContent`), then cancel and assert clean `cancelled` terminal. The reported negative-mutation proof (deleting the coverage message arm to force a failure) is consistent with the code's `task_materialization_requested` match at `stdio.rs:299-309`. This is a real, falsifiable oracle, not a tautology. |
| VF-opus item 6 (CLI wiring contradiction) | **Resolved** | `main.rs:24-25,103` implements `quality-artifacts recover|prune`; `docs/client-configuration.md:361-363` now says it "está integrada en `main.rs`", matching `README.md`/`architecture.md`. |

### 2. New finding

**P2 — ADR-064/065 acceptance status contradicts itself across the closure record.** `docs/adr/ADR-064-quality-job-seccomp-profile.md:5` and `ADR-065-coverage-target-volume.md:5`, plus `docs/adr/README.md:79-80`, now read "**Accepted** 2026-09-06 by the M3 orchestrator." But three documents that are supposed to state the current milestone status still say the opposite as of these same bytes:
- `docs/validation/M3-matrix.md:9,25` — "G2 … In progress … ADR-064 y ADR-065 continúan formalmente **Proposed** y requieren aceptación explícita del owner/orchestrator."
- `docs/validation/M3-07.md:134-135,268` — "ADR-064 | **Proposed** … aceptación formal pendiente" / "ADR-064 y ADR-065 continúan `Proposed`; su aceptación formal es del owner."
- `docs/implementation-status.md:167,172-173,258` — "Falta la aceptación formal de ADR-064/065" / "ADR-064 permanece Proposed hasta la aceptación formal del containment por el orchestrator."

D03 (which authored the "Accepted" text) never touched `M3-matrix.md`, `M3-07.md`, or `implementation-status.md`; W6 (which authored those three files) ran before D03 and correctly said "Proposed" at the time. Nothing here proves *who* actually authorized the ADR acceptance — this is the exact class of "same-change documentation" defect the brief already used to block closure on VF-01/F5. A reader of the closure matrix today cannot tell whether G2 is Done or not: the gating document (matrix) and the thing it gates on (the ADRs) disagree.

**This must be reconciled before Done**, either by:
(a) updating `M3-matrix.md`/`M3-07.md`/`implementation-status.md` to reflect the Accepted status if that acceptance was genuinely authorized, or
(b) reverting the ADR files/index to Proposed if the acceptance in D03 was not actually backed by an owner/orchestrator decision.

### 3. What must change before Done

1. Fix the ADR-064/065 status contradiction above.
2. Complete this re-review is itself G8's requirement — since I found the delta's fixes largely genuine but surfaced one new P2, another short confirmation pass after item 1 is fixed would let G8/G9 close cleanly.
3. VF-07/VF-08 remain intentionally-accepted residual gaps, not blockers — no action required unless the owner wants them closed.

### 4. Limitations of this pass
Read-only, no `cargo`/`docker`/`git` — I re-derived nothing by execution, only by reading the receipts, the ADR/doc text, and the new test source. I did not re-verify `LiveJobAuthority::revalidate` or the seccomp/mount-matrix code paths (V-SEC's and my prior pass already covered those and nothing in W6/D03/W5 touched them). I could not confirm who or what process actually authorized the ADR-064/065 "Accepted" text beyond the file contents themselves.