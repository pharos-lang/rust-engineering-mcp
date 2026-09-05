# M1-17 release qualification matrix

Updated 2026-09-04. Status: **In progress**. The historical macOS ARM64 full gate, actual MCP
Inspector 2.5.0 UI session, candidate-bound stock Codex client exercises and
independent review are complete. ADR-048 subsequently resolved the positive host,
artifact and no-catalog boundary. M1-15/M1-17 remain open only for final candidate
artifact, full/client/review evidence and publication. Product license, copyright
and source publisher/channel were resolved by ADR-047.

The accepted local binary SHA-256
`7a99038be57429e1db32c91d01772e7efd104691828253f45ed3bbb0e9330417`
is candidate-bound to `d024c7c72648206266f0d195ffc7040fb444eef6`: the
[source-equivalence receipt](../release/candidate/m1-17-source-equivalence.json)
rechecked all 238 selected inputs against that commit and the working tree, with
zero mismatches. It proves source-input equivalence, not a reproducible rebuild.

## Vertical evidence and dependencies

| Vertical | Evidence | Qualification boundary |
| --- | --- | --- |
| M1-10 local administration | [Signed import/recovery and gates](M1-10.md) | Integrated; native macOS/source-bound import, recovery and index rebuild |
| M1-11 catalog status | [Core/native status and postmerge](M1-11.md) | Integrated; actual E5/Lance load, degraded state and snapshot evidence |
| M1-12 crate search | [Core/native search and postmerge](M1-12.md) | Integrated; lexical/semantic/hybrid, fallback and ES/EN cases; no general relevance claim |
| M1-13 crate inspection | [Core/native inspection and postmerge](M1-13.md) | Integrated; authoritative SQLite facts, bounded paging and thirteen tools |
| M1-14 CLI/doctor | [Core/active/cancellation and postmerge](M1-14.md) | Integrated; historical 645 includes 644 tests plus one doctest; fresh gate reports those stages separately |
| M1-15 local candidate preparation | [Offline candidates](../release/offline-candidates.md), [postmerge receipt](../release/candidate/postmerge-receipt.json) | Historical source/full evidence; final ADR-048 core artifact remains pending |
| M1-16 experiments | [Completed pilot](M1-16.md), [utility report](../research/m1-16/measurement/REPORT.md), [retrieval benchmark](../research/m1-16/benchmark/REPORT.md) | Utility pilot saturated with no equivalence/value claim; retrieval run is bounded/descriptive only |
| M1-17 Inspector | [Actual Inspector qualification](M1-17-inspector.md) | 13/13 positive calls through persistent UI; cancellation notification observed; UI Resource read unqualified |
| M1-17 stock client | [Historical supplement](M1-17-codex-client.md), [final model-directed run](M1-17-codex-model.md) | Codex 0.153.0 model called the final binary, repaired E0502 to green and observed missing-runtime fail-closed |
| M1-17 final gate/review | [Historical full19](M1-17-final-gate.md), [final full v2](m1-17-final-gate-v2.json), [independent review](M1-17-review-opus.md), [disposition](M1-17-review-disposition.md) | Final 23-stage macOS ARM64 gate passed; final candidate review/disposition remains |

## Exact specification DoD (§M1, lines 4380–4397)

| Required evidence | Satisfying candidate receipt | Status |
| --- | --- | --- |
| Documented `inputSchema`/`outputSchema`, derived from Rust where possible | [Tool documentation](../tools.md); thirteen checked-in snapshots under `crates/mcp-server/tests/snapshots`; [canonical two-session client inventory](m1-17-codex-client/tool-inventory-canonicalization.json) | Satisfied at candidate source |
| Main responses use `structuredContent` | Per-tool protocol tests in the [full19 log](m1-17-final-gate/full.log); 13/13 actual [Inspector calls](M1-17-inspector.md) | Satisfied |
| Contract tests detect breaking schema changes | Thirteen snapshots plus Serde/JSON Schema validation in the candidate's 644-test stage; canonical digest `7c83911d…` stable across two client sessions | Satisfied |
| Linux/macOS/Windows portable core/protocol/catalog CI | [Public run 33928952807](public-ci-live-33928952807.json); historical full19 on macOS ARM64 | Satisfied for ADR-048 portable scope; no positive Linux/Windows claim |
| Real security tests for every advertised sandbox capability; tools blocked where guarantees are absent | M0-06/M1-01..09 adversarial gates, historical Docker/Rust stages and missing-runtime `SANDBOX_DENIED` | Satisfied for advertised macOS host plus approved Docker guest; other hosts fail closed |
| Structured output | Typed contract tests and actual Inspector/client responses above | Satisfied |
| Timeouts | [M1-06 active cancellation/EOF](M1-06.md), [M1-09 quality cancellation](M1-09.md), full19 Rust security | Satisfied for candidate source; Inspector evidence alone proves only client notification |
| Filesystem restrictions | [M0-04 roots/no-follow/races](M0-04.md), M1-01/M1-07, M1-10 authenticated import boundaries | Satisfied for the only positive host; Windows remains fail-closed |
| MCP operates with network disabled | M1-09, full19 Docker/Rust/audit/semantic gates with positive controls and enforced deny | Satisfied in qualified macOS/Docker configuration; no native Linux/Windows claim |
| Offline snapshot import | [M1-10](M1-10.md) authenticated import, limits, extraction, activation, antirollback and recovery | Satisfied on macOS candidate source |
| SQLite lexical and LanceDB semantic search | [M1-12 native E5/Lance/FTS](M1-12.md), Inspector hybrid call, fresh full19 semantic/search stages | Satisfied on macOS ARM64 |
| Correct fallback when LanceDB/embeddings are absent or invalid | M1-11/M1-12 degraded/fallback tests and fresh full19 | Satisfied |
| Catalog provenance/freshness | M1-11..13 plus actual Inspector/client catalog status | Satisfied |
| Integration tests | [Final full v2 receipt](m1-17-final-gate-v2.json) | Satisfied on final source with 23 passed stages |
| MCP Inspector tests | [Historical Inspector 2.5.0 UI qualification](M1-17-inspector.md) and [final receipt](m1-17-inspector-final.json) | Satisfied with the final CLI limitations recorded explicitly |

## Acceptance criteria traceability (§117, lines 5082–5107)

Section 117 is evaluated across the candidate evidence set; it does not require
all eighteen behaviors in one client session. The stock-model attempts remain
failed evidence and are never substituted for product or agent evidence.

| # | Agent can… | Candidate evidence and disposition | Status |
| ---: | --- | --- | --- |
| 1 | open a project | Inspector `rust.project.open`; stock Codex preflight and [repair receipt](m1-17-codex-client/repair-missing-receipt.json) | Satisfied |
| 2 | inspect configuration | Inspector `rust.project.inspect`; stock Codex preflight | Satisfied |
| 3 | modify code with its own capabilities | Eight model-authored repair submissions in [M1-16](M1-16.md) | Satisfied in bounded harness; stock-client model use unproven |
| 4 | execute `rust.check` | Inspector positive call; stock Codex E0502 then green check | Satisfied |
| 5 | receive structured diagnostics | Fresh candidate-bound stock Codex E0502 response; [M1-03](M1-03.md) E0502/E0106 cases | Satisfied |
| 6 | correct the error | M1-16 model-authored passing patches plus fresh client-observed external edit/check transition | Satisfied by combined evidence; no claim of one stock-model loop |
| 7 | execute Clippy | Inspector call; M1-16 standard quality-gate stages; [M1-05](M1-05.md) | Satisfied |
| 8 | execute tests | Inspector call; M1-16 quality-gate stages; [M1-06](M1-06.md) | Satisfied |
| 9 | execute audit | Inspector call; M1-16 quality-gate stages; [focused identified audit](m1-17-final-gate/audit-focused.json) | Satisfied, bounded by recorded local RustSec inputs |
| 10 | execute a quality gate | Inspector call and four M1-16 arm-B repair gates | Satisfied |
| 11 | do so without arbitrary shell | Exact thirteen-tool allowlist; no shell tool in Inspector; Codex host/code tools disabled; M1-16 broker allowlist | Satisfied for qualified workflows |
| 12 | stay inside the host-authorized filesystem closure | M0-04 adversarial roots/no-follow/race tests; per-session trusted roots | Satisfied for advertised macOS/approved Docker; other hosts fail closed |
| 13 | cancel processes | [M1-06](M1-06.md) observes active children and joined kill-tree cleanup; Inspector sent cancellation but did not prove active child state | Satisfied by product gate, not by Inspector alone |
| 14 | get clear errors when an external tool is missing | Fresh exact-binary stock Codex run returned structured `SANDBOX_DENIED`; the exact candidate CLI returned `unavailable/InvalidConfiguration` for nonexistent Docker; M1-14 cases | Satisfied |
| 15 | search crates locally through SQLite FTS5 | [M1-12](M1-12.md), full19 and Inspector lexical call | Satisfied |
| 16 | search by intent through LanceDB | M1-12 actual E5/Lance and Inspector hybrid call | Satisfied on macOS ARM64 |
| 17 | know snapshot/freshness used | M1-11..13 and actual Inspector/client catalog status | Satisfied |
| 18 | operate with network completely disabled | M1-09/full19 enforced network-deny tests with positive controls | Satisfied for advertised macOS host/approved Docker scope |

## ADR-048 disposition and final release gates

| Category | Open item | Why it remains open |
| --- | --- | --- |
| Resolved owner decision | Product license and copyright | ADR-047 selects `MIT OR Apache-2.0`; copyright IUMotion Labs; root license files apply to original code |
| Resolved owner decision | Source publisher/channel | IUMotion Labs via `pharos-lang/rust-engineering-mcp`; GitHub is the source channel and GitHub Releases the initial binary channel |
| Resolved delivery decision | GitHub artifact provenance | Keyless GitHub OIDC attestations; no long-lived repository signing secret |
| Resolved by ADR-048 | No official catalog in 0.1.0 | No catalog/trust/fixture/private or public production key is shipped; future publication needs a new decision |
| Excluded artifact limitation | Kanaria/E5/ORT/LanceDB redistribution | These remain visible for source builders but are prohibited from the core archive closure |
| Local artifact evidence | Core target notice inventory and SPDX SBOM | [Local archive receipt](../release/0.1.0-local-artifact-receipt.json) passed; tagged Actions rebuild must match the same contract |
| Final delivery gap | Artifact attestation/publication | Local install/smoke passed; GitHub OIDC attestation and Release remain |
| Resolved product metadata | Eight workspace crates previously had no product grant | All inherit `MIT OR Apache-2.0`; release-specific third-party notice work remains open |

M1-15 and M1-17 stay **In progress** until the final gates are recorded. A
declaration, `cargo deny` success or a historical local review copy is not artifact evidence.

## Additional release gates

| Gate | Evidence | Status |
| --- | --- | --- |
| Candidate binary/source link | [238-input equivalence receipt](../release/candidate/m1-17-source-equivalence.json) | Passed without rebuild; reproducible-build equality not claimed |
| Full local gate | [Final full v2 receipt](m1-17-final-gate-v2.json) | Passed all 23 stages on macOS ARM64 |
| Third-party clients | [Inspector history](M1-17-inspector.md), [final Inspector receipt](m1-17-inspector-final.json), [final stock Codex](M1-17-codex-model.md) | Final core binary repeated with Inspector 2.5.0 and a model-directed Codex 0.153.0 flow |
| Tool contract stability | [Two-session canonical inventory](m1-17-codex-client/tool-inventory-canonicalization.json) | Deep-equal; insertion-order historical digests superseded |
| Native semantics/performance | M1-12 actual paths; bounded retrieval benchmark and utility pilot | macOS ARM64 source evidence only; no general utility/quality claim |
| Licensing/notices | [Preparation](../release/preparation.md), [local archive receipt](../release/0.1.0-local-artifact-receipt.json) | Core archive passed; local/model/native assets excluded |
| Signed distribution | GitHub OIDC workflow | Final core attestation pending; no official catalog distribution |
| Independent final review | [Opus 5 read-only review](M1-17-review-opus.md) and [principal disposition](M1-17-review-disposition.md) | Historical review completed; model-authored, not human; final candidate requires a new review and disposition |
| Closure | This matrix and board | **In progress**; source is public, no binary release is published |

## Native matrix and capability boundaries

| Platform | Observed status | Required before qualification |
| --- | --- | --- |
| macOS 26.6.2 ARM64/APFS | Only positive host; source-bound local evidence | Final core artifact/full/client/review evidence pending |
| Linux x86_64 | Portable/fail-closed CI only | No positive capability or 0.1.0 artifact advertised |
| Windows x86_64 | Portable/fail-closed CI only | No reparse-safe positive adapter or 0.1.0 artifact advertised |
| Linux ARM64, macOS x86_64, Windows ARM64 | Not advertised | Outside 0.1.0 artifact matrix |
| Docker Linux ARM64 guest | Approved execution image and adversarial project-code tests | Never counted as native OS/library distribution qualification |

Spec §M1 names three OS families for portable CI. ADR-048 limits positive capability
and artifact claims. A green portable runner is never relabeled as native sandbox,
filesystem, model or distribution qualification.
