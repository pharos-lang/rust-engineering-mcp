# M1-16 prospective experiment protocol v1

Status: design only, no participant runs, results, success claims or costs measured.
This freezes a proposed protocol for principal review, not an implemented runner.
Any amendment must precede measured runs and carry a new version with its reason.
M1-16 remains pending until M1-01..15 and the prerequisites below are satisfied.

## Questions and arms

Test both specification §94 hypotheses: whether structured Cargo/rustc evidence
improves repair, and whether local hybrid catalog evidence improves offline crate
selection. Use GPT-5.6 Sol, explicit identifier `gpt-5.6-sol`, medium effort in both
arms. Availability of that exact model in the installed CLI is unverified: reject
an unavailable model, never substitute silently. Claude Sonnet5 is reserved for
independent review, not an alternative experimental treatment.

Each pair starts from byte-identical fresh workspaces with identical allowed
read/edit closure, prompt, immutable runtime/dependency/catalog/model inputs and
budgets. Neither arm may execute edited or untrusted Rust on the host. A trusted
experiment controller performs validation through the same calibrated Execution
Gateway and fixed commands in both arms. Agent shell/code-execution, network tools,
other MCPs, plugins, inherited project instructions and background agents must be
disabled or contained by enforced controller policy; a prompt-only prohibition
does not qualify. The controller/read-edit broker is not implemented yet.

- Arm A, no Rust Engineering MCP: controller returns bounded raw gateway stdout,
  stderr and exit/timeout evidence for the same fixed validation operations. For
  selection tasks it exposes the same frozen fact corpus as bounded plain text,
  without MCP search/ranking. No arbitrary shell API is introduced.
- Arm B, Rust Engineering MCP: controller exposes the thirteen M1 tools and
  authorized Resources; validation feedback comes only from those tools. It does
  not additionally supply raw gateway feedback outside the MCP result/resources.

Same output byte limits and execution policies apply. Tool formatting/search are
the treatment; different command power, Internet access or source corpus are not.
LLM service communication belongs to the trusted host, separate from the
network-denied project runtime. Do not call the entire experiment air-gapped.

## Corpus freeze and missing tasks

Baseline: commit `463bab799da4b2cb3999f6f083d91e2dbd8641f9`,
`fixtures/corpus-sha256.json`, fixture locks and documented oracles in
[fixtures README](../../fixtures/README.md). Freeze a separate experiment manifest
with every initial source, prompt, lock, reference repair and hidden oracle hash
before running. Existing validation fixtures are seeds, not a completed benchmark.

| Task ID | Seed / initial fault | Objective oracle and readiness |
| --- | --- | --- |
| R01 ownership | `borrow-error`, E0502 | Preserve intended reads/results and eliminate E0502; hidden behavior assertions/reference repair still to author |
| R02 lifetime | `lifetime-error`, E0597 | Return/use valid owned or sufficiently lived value; hidden behavior assertions/reference repair still to author |
| R03 Clippy | `clippy-warning`, useless_vec | Preserve output, strict Clippy and tests pass; hidden behavior assertions still to author |
| R04 features | `feature-conflict`, left+right | Task prompt requires left behavior and excludes right; inspect selected features plus hidden behavior test; experiment prompt/oracle missing |
| R05 async Send | Missing fixture | Future held across await contains Rc; require spawnable Send future and exact output; pinned fixture, test and reference repair missing |
| R06 API migration | Missing fixture | Code uses removed function against a fixed offline dependency version; require replacement API and behavior tests without changing dependency version; fixture/cache missing |
| R07 regression | Missing fixture | Deliberate boundary arithmetic bug with existing public tests and hidden boundary cases; require both to pass without removing tests; fixture missing |
| R08 vulnerable dependency | Existing RSA fixture is audit-only, not compilable | Buildable frozen vulnerable/fixed dependency pair, safe API behavior test and immutable RustSec snapshot; replacement fixture/cache/repair oracle missing; never run exploit |
| S01..S04 catalog selection | Missing real frozen task corpus | Four intents: offline parsing, Unicode normalization, deterministic serialization, bounded async channel; paired ES/EN prompts, pinned facts and relevance labels reviewed before running |

Use no hostile containment fixtures as experimental tasks. After drafting, have
the principal independently verify all hidden oracles and review the seeds for
leaked repairs. Selection labels require concrete MSRV/license/API constraints,
accepted crate/version identities and rejected counterexamples grounded in the
snapshot. Text alone is not evidence that those crates/APIs are available.

## Schedule, budgets and outcomes

Planned study: eight repair tasks and eight selection prompts (four intents in two
languages), three independent repetitions per item: 48 pairs / 96 runs. Repetition
1 uses A then B, repetition2 B then A, repetition3 alternates by task ID parity.
Record all ordering; fresh sessions and workspaces per run, no resumption across
arms, no answer sharing. This is a small exploratory paired study, not population
proof; repetitions/language variants are clustered by task family in reporting.

Per run: 15 minutes wall time, at most six submitted candidates and six agent-
requested validation cycles, 30,000 reported output tokens stopping at completed
turn boundaries. This token threshold is not a hard in-flight cap; retain any
overshoot. Controller wall deadline and candidate limits are hard. Gateway command
deadlines retain their production limits, including the quality-gate ceiling.
Run an independent final oracle outside the agent's feedback budget. No invisible
extra repair attempt is allowed after final evaluation.

Candidate1 is the first submitted complete patch/selection. Record its independent
oracle outcome as first-attempt success. One repair loop is an additional submitted
candidate after feedback: candidate2 has one loop, candidate6 five. Record final
success, attempts used and loops to success; failures are censored at their limit,
not assigned fictional successful loop counts. A request for diagnostic evidence
without a submitted candidate counts as validation usage, not a repair loop.

Repair success requires immutable hidden behavioral tests, requested task-specific
conditions, fmt/check/strict Clippy/test/audit final gate, no unauthorized changes
or test deletion, and no new validated security finding. Selection success requires
accepted identity plus every fixed constraint and source/freshness references;
if a task claims working integration, it must also compile/test through the gateway.
Distinguish model/task failure from infrastructure failure; keep both denominators,
never discard an unfavorable outcome. Any replacement infrastructure run repeats
the whole pair and retains original records with its exclusion reason.

Report paired first/final success deltas, attempts/loops, input/cached-input/output
tokens, wall and gateway time, tool counts, validated security findings and each
final gate stage. Report task-family results and uncertainty, not just one average.
The hypothesis is unsupported when MCP does not improve success or loops under
these budgets, or improvements trade against material regressions; publish null
and negative observations with the same artifacts. Never infer token savings from
shorter tool output alone.

## Capture and prerequisites

Local inspection on 2026-09-04 found Codex CLI0.153.0 and `codex exec --help`
options `--json`, `--model`, `--ephemeral`, `--ignore-user-config` and
`--output-last-message`. Official [non-interactive documentation](https://learn.chatgpt.com/docs/non-interactive-mode)
was opened and describes JSONL item/turn events and `turn.completed.usage` fields.
Capture stdout JSONL and stderr separately plus exact CLI/config/model identities.
Sum usage from distinct completed turns; retain cached-input separately rather than
adding it again to input. Missing usage after interruption is unknown, not zero;
preserve partial records and report token coverage. Documentation/help inspection
does not verify model access, event completeness or policy enforcement on this host.

Before measurement, build and review the enforced controller, freeze its hash and
prompts/oracles, verify exact model availability and usage fields with a disjoint
smoke task, calibrate cleanup and isolation in both arms, and record the authorized run budget before measurement.
Do not obtain account credentials by copying them into fixtures/transcripts. Record
source/catalog/model SHA, snapshot freshness, runtime image, toolchain and capability
receipt, session IDs, monotonic timestamps, patches, evidence and final oracle for
each run. Redact credentials while preserving evidence hashes and explicit omissions.

An independent reviewer receives anonymized candidate patches, oracle outcomes and
security evidence before aggregate interpretation. No reviewer implements repairs.
The current file supplies no numerical experiment outcomes.
