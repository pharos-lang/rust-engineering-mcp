# M1-17 release qualification matrix

Updated 2026-09-05. Status: **Done**. The macOS ARM64 full gate, MCP Inspector
2.5.0, model-directed stock Codex flow and independent review are complete. ADR-048
defines the positive host, artifact and no-catalog boundary. Protected public CI,
tag-bound artifact qualification, attestations, independent re-download/smoke and
the stable release are recorded in the [public release receipt](m1-17-public-release.json).

The final local client-qualified binary SHA-256 is
`ebcb292c71d863aabb900874651230d0a16d5c93f68da84afb84bd89f4977edf` and the
[local artifact receipt](../release/0.1.0-local-artifact-receipt.json) binds it to
source commit `a6ea6b782e57271c01885bd147e5b66835ed9f8d`. The independently built public
binary SHA-256 is `8f6f8c754ae3bde6cc2089ffb5c6360e5c9ebb61af7f022477ee10a30ed336ef`
and its [public receipt](m1-17-public-release.json) binds it to tagged commit
`452acdbf3a634d2cc0b9d153db09718237625b9d`. Byte equality between the local and
Actions builds is not claimed; each evidence set qualifies its own bytes.

## Vertical evidence and dependencies

| Vertical | Evidence | Qualification boundary |
| --- | --- | --- |
| M1-10 local administration | [Signed import/recovery and gates](M1-10.md) | Integrated; native macOS/source-bound import, recovery and index rebuild |
| M1-11 catalog status | [Core/native status and postmerge](M1-11.md) | Integrated; actual E5/Lance load, degraded state and snapshot evidence |
| M1-12 crate search | [Core/native search and postmerge](M1-12.md) | Integrated; lexical/semantic/hybrid, fallback and ES/EN cases; no general relevance claim |
| M1-13 crate inspection | [Core/native inspection and postmerge](M1-13.md) | Integrated; authoritative SQLite facts, bounded paging and thirteen tools |
| M1-14 CLI/doctor | [Core/active/cancellation and postmerge](M1-14.md) | Integrated; historical 645 includes 644 tests plus one doctest; fresh gate reports those stages separately |
| M1-15 release | [Offline candidates](../release/offline-candidates.md), [local core receipt](../release/0.1.0-local-artifact-receipt.json), [public receipt](m1-17-public-release.json) | Historical candidates retained; final tagged core artifact published and independently verified |
| M1-16 experiments | [Completed pilot](M1-16.md), [utility report](../research/m1-16/measurement/REPORT.md), [retrieval benchmark](../research/m1-16/benchmark/REPORT.md) | Utility pilot saturated with no equivalence/value claim; retrieval run is bounded/descriptive only |
| M1-17 Inspector | [Actual Inspector qualification](M1-17-inspector.md) | 13/13 positive calls through persistent UI; cancellation notification observed; UI Resource read unqualified |
| M1-17 stock client | [Historical supplement](M1-17-codex-client.md), [final model-directed run](M1-17-codex-model.md) | Codex 0.153.0 model called the final binary, repaired E0502 to green and observed missing-runtime fail-closed |
| M1-17 final gate/review | [Historical full19](M1-17-final-gate.md), [final full v2](m1-17-final-gate-v2.json), [final independent review](../reviews/M1-closure-final-claude-opus-5.md), [public receipt](m1-17-public-release.json) | Final 23-stage gate and Opus 5 review passed; zero P0/P1 and release conditions satisfied |

## Exact specification DoD (§M1, lines 4380–4397)

| Required evidence | Satisfying candidate receipt | Status |
| --- | --- | --- |
| Documented `inputSchema`/`outputSchema`, derived from Rust where possible | [Tool documentation](../tools.md); thirteen checked-in snapshots under `crates/mcp-server/tests/snapshots`; [canonical two-session client inventory](m1-17-codex-client/tool-inventory-canonicalization.json) | Satisfied at candidate source |
| Main responses use `structuredContent` | Per-tool protocol tests in the [full19 log](m1-17-final-gate/full.log); 13/13 actual [Inspector calls](M1-17-inspector.md) | Satisfied |
| Contract tests detect breaking schema changes | Thirteen snapshots plus Serde/JSON Schema validation in the candidate's 644-test stage; canonical digest `7c83911d…` stable across two client sessions | Satisfied |
| Linux/macOS/Windows portable core/protocol/catalog CI | [Final public receipt](m1-17-public-release.json), run `33948778666`; full gate on macOS ARM64 | Satisfied for ADR-048 portable scope; no positive Linux/Windows claim |
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
| 3 | modify code with its own capabilities | Eight model-authored repair submissions in [M1-16](M1-16.md) plus the final stock Codex repair | Satisfied; final stock model edited only the authorized fixture source |
| 4 | execute `rust.check` | Inspector positive call; stock Codex E0502 then green check | Satisfied |
| 5 | receive structured diagnostics | Fresh candidate-bound stock Codex E0502 response; [M1-03](M1-03.md) E0502/E0106 cases | Satisfied |
| 6 | correct the error | M1-16 passing patches plus final stock Codex E0502 edit and green recheck | Satisfied in one candidate-bound stock-model loop |
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
| Local and public artifact evidence | Core target notice inventory and SPDX SBOM | [Local archive receipt](../release/0.1.0-local-artifact-receipt.json) and [tagged public receipt](m1-17-public-release.json) passed the same contract |
| Final delivery | Artifact attestation/publication | GitHub OIDC attestations, independent download/smoke and stable Release passed |
| Resolved product metadata | Eight workspace crates previously had no product grant | All inherit `MIT OR Apache-2.0`; release-specific third-party notice work remains open |

M1-15 and M1-17 are **Done** because the final gates and public bytes are recorded.
The closure does not rely on a declaration, `cargo deny` alone or a historical copy.

## Additional release gates

| Gate | Evidence | Status |
| --- | --- | --- |
| Candidate binary/source link | [238-input equivalence receipt](../release/candidate/m1-17-source-equivalence.json) | Passed without rebuild; reproducible-build equality not claimed |
| Full local gate | [Final full v2 receipt](m1-17-final-gate-v2.json) | Passed all 23 stages on macOS ARM64 |
| Third-party clients | [Inspector history](M1-17-inspector.md), [final Inspector receipt](m1-17-inspector-final.json), [final stock Codex](M1-17-codex-model.md) | Final core binary repeated with Inspector 2.5.0 and a model-directed Codex 0.153.0 flow |
| Tool contract stability | [Two-session canonical inventory](m1-17-codex-client/tool-inventory-canonicalization.json) | Deep-equal; insertion-order historical digests superseded |
| Native semantics/performance | M1-12 actual paths; bounded retrieval benchmark and utility pilot | macOS ARM64 source evidence only; no general utility/quality claim |
| Licensing/notices | [Preparation](../release/preparation.md), [local archive receipt](../release/0.1.0-local-artifact-receipt.json) | Core archive passed; local/model/native assets excluded |
| Signed distribution | [Public release receipt](m1-17-public-release.json) | Three subjects verified against tag, source commit and signer workflow; no official catalog distribution |
| Independent final review | [Final Opus 5 read-only review](../reviews/M1-closure-final-claude-opus-5.md) | Accepted/ready; model-authored, not human; zero P0/P1 and P2 publication conditions resolved |
| Closure | This matrix, board and [release](https://github.com/pharos-lang/rust-engineering-mcp/releases/tag/v0.1.0) | **Done** |

## Native matrix and capability boundaries

| Platform | Observed status | Required before qualification |
| --- | --- | --- |
| macOS 26.6.2 ARM64/APFS | Only positive host; source-bound local evidence and published core artifact | Qualified for the bounded 0.1.0 contract |
| Linux x86_64 | Portable/fail-closed CI only | No positive capability or 0.1.0 artifact advertised |
| Windows x86_64 | Portable/fail-closed CI only | No reparse-safe positive adapter or 0.1.0 artifact advertised |
| Linux ARM64, macOS x86_64, Windows ARM64 | Not advertised | Outside 0.1.0 artifact matrix |
| Docker Linux ARM64 guest | Approved execution image and adversarial project-code tests | Never counted as native OS/library distribution qualification |

Spec §M1 names three OS families for portable CI. ADR-048 limits positive capability
and artifact claims. A green portable runner is never relabeled as native sandbox,
filesystem, model or distribution qualification.
