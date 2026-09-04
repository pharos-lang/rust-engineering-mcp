# M1-16 protocol v2 — prospective 24-run pilot

Status: completed exactly once under the approved freeze; see the
[measured validation](M1-16.md). This text was the principal-approved design
amendment before measurement. It supersedes the explicitly
amended design choices of [v1](M1-16-protocol-v1.md); preserves
v1 as the historical96-run proposal. No claim that24 runs complete that study or
close M1-16/M1. Setup, echo, model-availability and timeout qualification runs are
disjoint infrastructure checks excluded from all utility denominators.

## Questions, scope and amendment rationale

Retain both spec§94 hypotheses: structured Cargo/rustc evidence for correct repair
with fewer iterations, and SQLite+LanceDB local catalog evidence for crate/API
selection without participant Internet acquisition. Exact model gpt-5.6-sol/medium
is fixed in both arms; no provider/model fallback. One repetition per item.

The currently authored, source-reviewable offline corpus contains four std-only
repair tasks and four selection intents in EN/ES. Eight repair fixtures and three
repetitions of v1 are not ready. Reduce prospectively to12 pairs/24 runs rather
than fabricate missing fixtures or silently treat setup success as utility evidence.
This is an exploratory feasibility pilot with8 task families, not24 independent
problems and not a powered confirmatory study. Language variants share a family.

Repair R01 ownership/E0502, R02 owned lifetime/E0597 and R03 useless_vec retain the
v1 concepts. R04 is now an arithmetic-overflow ceiling-division regression, NOT
v1's feature-conflict left/right task. Record this substitution explicitly.
V1 R05 async-Send, R06 API migration, R07 broader regression and R08 vulnerable
buildable dependency repair are omitted; current R04 overlaps the regression
concept but does not complete the missing v1 task. All repair projects are std-only;
no claim about dependency, async, feature or vulnerability repairs follows.

Selection corpus:15 crates/16 recorded versions, four intents TOML+Serde, Unicode
NFC, default JSON map key order, bounded async MPMC. Captured registry metadata is
version-specific and non-atomic; projection source_id explicitly says research.
Dependencies/advisories unacquired remain omitted rows with warnings, not claims of
absence. Authored API annotations cite retained source hashes in descriptions;
the identical descriptions must be available to both arms.

## Arms and equal authority

A uses the controller's closed reads/edits and fixed calibrated gateway operations;
repair feedback is bounded raw stdout/stderr/exit metadata. Selection receives only
the plain projection of the same CrateRecord facts, annotations and provenance,
with authoritative snapshot fingerprint added after actual bundle emission. No
extra README/raw source, labels, hidden oracles or reference patch is available.

B exposes the unchanged13 M1 MCP tools plus authorized resources through fixed
dynamic mappings. Facts remain SQLite-authoritative; hybrid requires verified
local E5/Lance identities, frozen equally across runs. If unavailable before a pair,
stop as infrastructure failure rather than measuring a silently lexical treatment.
Record actual requested/effective modes and fallback on every search; an in-run
fallback is retained as observed treatment delivery, never concealed or discarded.
B receives no additional raw validation stream outside its MCP results/resources.

Both use the same outer byte caps, command categories, policy, runtime image/limits,
initial files and snapshot corpus. Dynamic callbacks, including reads, have a64-call
global admission cap; this is NOT the six-candidate or six-validation limit.
The parent broker separately enforces at most6 complete candidate submissions and
6 requested validation cycles (compound subcommands counted and recorded).
It must prevent metadata/read aliases from gaining extra execution authority.

Installed app-server/code-mode host provide a source-reviewed narrow V8 API with
no thread environment or environment overrides. Disable native shell/patch through
no-environment guards, MCP/apps/plugins/browser/web, skills/orchestrator, memories,
hooks and multi-agent (including agents.enabled=false). Keep exact config/model
checks, empty instructionSources and clean child environment. Code-mode is NOT an
OS sandbox claim: V8/no-import/callback confinement is the stated boundary.
Enumerate any clock/user-input/plan/registry-metadata utilities identically; reject
unadmitted server requests. LLM host service traffic is not air-gapped project I/O.

## Prospective order and budgets

Freeze this pair order before observing outcomes. Odd repair IDs start A; even
start B. Selection uses intent parity, reversing between EN and ES so neither
language is uniformly first in one arm. Pair members run serially on fresh states.

| Pair | Item | First → second |
| --- | --- | --- |
| 1 | R01 | A → B |
| 2 | R02 | B → A |
| 3 | R03 | A → B |
| 4 | R04 arithmetic | B → A |
| 5 | S01-en | A → B |
| 6 | S01-es | B → A |
| 7 | S02-en | B → A |
| 8 | S02-es | A → B |
| 9 | S03-en | A → B |
| 10 | S03-es | B → A |
| 11 | S04-en | B → A |
| 12 | S04-es | A → B |

Per run:900s admission window,6 candidates,6 validation cycles,64 dynamic calls,
30,000 reported output tokens as an observed stopping threshold. Stop new admission
and signal cancellation at wall/token limits; join all active gateway/controller
work. V1's “hard wall” statement is amended: native/synchronous handler cleanup is
cooperative and may extend elapsed time. Record candidate-window and cleanup time
separately; do not kill detached jobs and claim successful cleanup. Token updates
replace cumulative totals; cached input is a subset of input, not additive.
Interrupted usage is partial_reported_before_interruption or unknown, never zero.
The upper planned candidate-window allocation is24×15min=6h, excluding setup and
oracle/cleanup time; this is a budget calculation, not measured runtime or cost.

## Success and oracle protocol

Archive every complete candidate with hashes. Candidate1 is the first submission,
not the first validation call. Additional candidates define repair loops. Reads
and diagnostic requests alone do not advance the candidate index. Evaluate first
and final candidates independently AFTER the participant run (or use deduplicated
identical candidate hashes), with hidden results never returned to the participant.
This reconciles first-attempt scoring with hidden tests mounted only at oracle time.

Repair: exact public signature and declared behavior, only src/lib.rs editable,
immutable Cargo manifests/locks, no hidden/reference access, no tests added to src,
dependency/manifest modifications prevented by the broker; unsafe code, source tests,
lint suppression, signature changes and hardcoded oracle cases are disqualifying
conditions checked in independent source review. The broker does not parse Rust or
claim those semantic properties merely from a successful submission. Final
oracle performs fmt/check/strict Clippy/tests with the trusted hidden harness through
the approved gateway. The std-only audit is also validated via the MCP snapshot
outside participant feedback for BOTH arms. Report each stage independently;
no-vulnerability snapshot outcome is not a universal security proof.
Reference patches are examples, not byte-equality acceptance. Any passing alternative
with equivalent required behavior and authorized edits is accepted.

Selection: accepted exact identity plus declared MSRV/license constraint, task API
caveat, corpus date, authoritative snapshot_fingerprint and provenance.source_id.
Use the revised overlay selection/prompts/*.txt. Mandatory raw README hashes are
removed because MCP may not expose those fields; embedded annotation hashes are
available symmetrically but optional. No transitive/integration/legal/safety claim
is accepted as inferred from metadata. The evaluator must amend the v1
source_hash_and_snapshot_date_required label field into the explicitly stated v2
snapshot/provenance check; do not silently reuse that old predicate.

Task/model failures and infrastructure failures stay separately counted. Keep
failed/partial logs, interruption, quota/errors, mismatched identity and cleanup
state. A replacement infrastructure attempt requires principal disposition and
repeats the whole pair while retaining original records; never cherry-pick a side.

## Source review and findings before freeze

- P1 corrected: repair/R01/hidden/behavior.rs:5, R02:5-6 and R04:80 failed trusted
  rustfmt --check. Mounted before final fmt, they would reject even reference
  repairs. Formatting only was authorized and applied; references already passed.
  Retained format_and_hash.py normalizes all hidden/reference files and regenerates
  SHA256SUMS.json. No behavior or reference algorithm changed. Initial/reference
  gateway execution was subsequently qualified (20 executions below); formatter alone was not compilation proof.
- P1 historical pending binding (resolved by emitted projection below): target/m1-16-catalog/baseline-projection.json:1-9 contains
  staging provenance but no actual snapshot_fingerprint. Add only the fingerprint
  and evidence/provenance observed from the emitted verified snapshot to the plain
  participant payload; align integrity/freshness meaning with B, do not invent it
  from records.json SHA. Emit-side worker has been asked for the identity contract.
- P2 corrected prompt overlay: selection/S01-en.txt:2 (same requirement in all
  eight originals) required raw source hashes. New selection/prompts/ files demand
  accessible snapshot/provenance instead; originals are retained as v1 artifacts.
  tasks-and-labels.json:199/403/607/811 still names the old source-hash predicate;
  principal-owned evaluator must explicitly bind to the v2 rule before measuring.
- P2 historical readiness gap (subsequently qualified below): repair/R01..04/oracle.json:2-3,21-22 correctly report unexecuted
  initial/reference gates. Verify expected initial faults and reference passes on
  the frozen approved runtime before freeze; R03/R04 lints may vary with toolchain.
- Limitation: R01 hidden tests cover4 vectors; R02 covers4 strings; R03 covers9
  integers within the prompt's domain; R04 covers72 boundary combinations. These
  discriminate obvious incorrect repairs but do not prove the universal domains.
  Inspect final patches for hardcoded cases; keep reference/hidden files outside
  participant closure. R04 initial may also trigger a Clippy diagnostic, not only
  hidden overflow; report actual calibrated trigger rather than presupposing it.

Read-only formatting check reproduced the original failure; post-format check of
all8 hidden/reference files passed. Python verified the72 R04 precomputed cases
against independent arbitrary-precision ceiling arithmetic. No Rust execution,
Docker, agent utility inference or metric measurement was performed in this review.

## Freeze prerequisites and reporting

Principal approves this amendment and validates all oracles/constraints, then
freezes exact source commit, controller/broker binaries/scripts, image/calibration,
RustSec snapshot, project locks, model/tokenizer/runtime/index/snapshot identities,
prompts, labels, queries and reference/hidden hashes. Include generated overlay and
normalization recipe in the refreshed corpus manifest. Reject initial state drift.
No credentials/config secrets enter prompts, payloads or logs. Fresh sessions and
byte-identical initial workspaces prevent carryover between pair members.

Report paired first/final success counts/deltas, candidates/loops, validation and
read/tool counts, input/cached/output usage with coverage, elapsed/cleanup/gateway
time, per-stage oracle status, treatment fallback and validated security findings.
Keep family/language tables and all observations; single repetition does not estimate
within-item variance. Do not infer savings from smaller tool payloads or claim
causal/general gains beyond this tiny authored corpus. Null/negative results are
reported equivalently. Independent anonymized review precedes interpretation.
Acceptance of this protocol does not itself close M1-16, supply96-run evidence, or
qualify clients/platforms/licensing/distribution.

## Principal amendment disposition

The specification prescribes two hypotheses and reproducible measured metrics, not
a minimum sample size. V1 was a prospective local proposal, not an owner-selected
96-run requirement. V2 defines the actual bounded pilot and its limitations before
any utility result exists; no population-effect or completion of the earlier
proposal can be inferred. A negative or inconclusive result is an allowed outcome.
M1-15 distribution approvals remain pending; private research on source-matched
local review binaries does not depend on publication or a product-license grant.

Execution budget counts one admitted request for each check/fmt/clippy/test/audit,
project.inspect, toolchain.inspect or diagnostics.explain. Quality bundles its
fixed stages into one request in both arms; stage count/time is retained separately.
Catalog/read/edit requests count toward64 total, not six validation requests.
Discovery occurs before the participant window and is recorded as setup. Fixed
raw commands use30s/256KiB per stream; test input above30s is rejected in B. Both
arms require strict Clippy. Admitted-tool object requests with invalid/missing/extra fields receive retryable
denials without rewriting public MCP schemas. Unknown callback names, non-object
arguments and unadmitted server requests stop the run, as disclosed to both arms.
The unequal number of callback names can affect this attrition; retain and report
these outcomes instead of claiming retryability for every invalid request. Raw rustc explanation uses the same
closed diagnostic code and fixed gateway operation.

## Qualification completed before participant freeze

-20 raw gateway executions reproduced four expected initial failures and16 passing
  reference stages (fmt/check/strict Clippy/test), with source hashes and joined
  cleanup. Four real SDK audits additionally observed one workspace package and
  zero registry dependencies in each reference. The advisory input is a declared
  one-record research fixture, sufficient for this zero-third-party closure; not
  a complete advisory database or vulnerability-repair experiment.
- SDK qualification executed42 calls across all13 tools in persistent sessions,
  read and hash-verified a real E0502 Resource, and cancelled raw/MCP while an
  owned Docker Cargo test execution was observed. Both owners joined, and no
  owned container/volume remained. No forced stop was accepted as cleanup.
- Historical14 participant/22 broker tests and13 evaluator tests passed;
  resumed corrections raise these to33/26/15, plus8 analyzer and13 driver+1bundle tests. A real post-review
  model echo validated the changed bounded transport and cleanup. Source inspection
  found/fixed blocked writer lock cancellation, an ignored server-join flag and
  premature abort of retryable arguments. Freeze also requires exact Codex and
  code-mode-host binary hashes. Independent Opus follow-up and principal dispositions are complete; exact freeze remains required.
- Native benchmark completed one session,76 actual calls,8 frozen ES/EN queries
  across3 modes with3 warm repetitions. This deterministic retrieval qualification
  is separate from the paired-agent utility outcomes.

Evidence is retained in [research artifacts](../research/m1-16/README.md).

## Resumed qualification corrections before freeze

The first post-restart exact-feature preflight stopped before a model turn: the
old receipt had intentionally retained a filtered map. The corrected source-based
guard checks all41 actual config keys:38false, code_mode_host=true,
skip_host_skill_discovery=true and network_proxy=null without overriding host
proxy policy. Binary/source pins and no-environment guards remain the access
boundary; this is not an authoritative complete native tool inventory.

App-server cwd is now a fresh0700 /private/tmp directory outside evidence/corpus.
Process-inspection uncertainty, forced stop or missing joins are separate cleanup
failures; they stop the series. Committed candidate results survive cancellation
before response delivery. Queue capacity is byte-bounded, with retained stop
reasons, task/infra status, prompts and tool-declaration hashes. Empty-environment
ephemeral sessions skip thread persistence/state DB in pinned Codex source; no
claim is made about normal service/auth/usage metadata or provider retention.

Driver error acknowledgements distinguish execution/server joins from sticky
cleanup uncertainty; arbitrary exit1 never proves cleanup. A read-only observer
checks product-labelled Docker containers/volumes before and after each run and
repair oracle. No observer removes resources or signals project jobs. Input drift
or preexisting execution objects stop subsequent measurement.

Shared caps are512KiB encoded callbacks,1MiB driver lines and16MiB cumulative
transport/logs. Raw stages retain256KiB per stream under30s limits; production MCP
keeps its existing envelope/resource budget semantics. These caps do not imply
equal information content. A verbose response can censor either arm differently:
record that as infrastructure failure and retain the run, rather than truncate
arbitrary structured envelopes or silently discard failures. Corpus calibration
must show the actual compound quality path fits before freeze. Docker image/OS
page/provider prompt caches remain timing confounders despite counterbalancing;
per-execution Cargo target volumes are fresh in both arms.

See [principal disposition](../research/m1-16/qualification/principal-disposition.md)
for all C/H/M/L findings, including rejected premises and remaining qualifications.

Before each participant window in BOTH arms, a real SDK catalog.status must report
available catalog/model/index with exact frozen identities and index document
count. A uses a temporary setup MCP session which is joined before inference; B
uses its retained session. This setup is recorded outside model/validation budgets.
It ensures availability before the first pair member as well as before its partner;
subsequent in-run fallback remains an observed result. The evaluation corpus date
is derived from the frozen projection provenance observed_at in UTC, not a literal
in the evaluator. Missing labels/dates reject before creating evaluation output.
The Docker observer deliberately rejects any stderr warning as uncertainty; it
may stop a run conservatively, never turn an unknown observation into absence.
