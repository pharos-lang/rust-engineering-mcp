# M1-17 release qualification matrix

Updated 2026-09-04. Status: the fresh macOS ARM64 full gate, actual MCP
Inspector 2.5.0 UI session, candidate-bound stock Codex client exercises and
independent final review are complete. **M1-17 and M1 remain Blocked** by the
native OS/architecture matrix, unresolved third-party/native licensing evidence,
and production catalog-key custody listed below. Product license, copyright and
source publisher/channel were subsequently resolved by ADR-047.

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
| M1-15 local candidate preparation | [Offline candidates](../release/offline-candidates.md), [postmerge receipt](../release/candidate/postmerge-receipt.json) | Local preparation complete; release remains blocked by the two categories below |
| M1-16 utility experiment | [Completed pilot](M1-16.md), [report](../research/m1-16/measurement/REPORT.md) | Done for the frozen 24-run pilot; saturated endpoint, null success difference and no equivalence/value claim |
| M1-17 Inspector | [Actual Inspector qualification](M1-17-inspector.md) | 13/13 positive calls through persistent UI; cancellation notification observed; UI Resource read unqualified |
| M1-17 stock client | [Codex supplement](M1-17-codex-client.md) | Direct client tool/Resource and repair/missing-runtime mechanics passed; model-driven stock-client use remains unproven |
| M1-17 final gate/review | [Fresh full19](M1-17-final-gate.md), [independent review](M1-17-review-opus.md), [disposition](M1-17-review-disposition.md) | macOS ARM64 passed; review completed and surviving blockers are explicit |

## Exact specification DoD (§M1, lines 4380–4397)

| Required evidence | Satisfying candidate receipt | Status |
| --- | --- | --- |
| Documented `inputSchema`/`outputSchema`, derived from Rust where possible | [Tool documentation](../tools.md); thirteen checked-in snapshots under `crates/mcp-server/tests/snapshots`; [canonical two-session client inventory](m1-17-codex-client/tool-inventory-canonicalization.json) | Satisfied at candidate source |
| Main responses use `structuredContent` | Per-tool protocol tests in the [full19 log](m1-17-final-gate/full.log); 13/13 actual [Inspector calls](M1-17-inspector.md) | Satisfied |
| Contract tests detect breaking schema changes | Thirteen snapshots plus Serde/JSON Schema validation in the candidate's 644-test stage; canonical digest `7c83911d…` stable across two client sessions | Satisfied |
| Linux/macOS/Windows core/protocol/catalog CI | [Fresh full19](M1-17-final-gate.md) on macOS ARM64 only | **Blocked:** native Linux and Windows receipts absent |
| Real security tests for every advertised sandbox capability; tools blocked where guarantees are absent | M0-06/M1-01..09 adversarial gates, [fresh Docker/Rust security stages](M1-17-final-gate.md), and fresh missing-runtime `SANDBOX_DENIED` in [client receipt](m1-17-codex-client/repair-missing-receipt.json) | Satisfied for qualified macOS host plus approved Docker guest; **blocked for absent native platforms** |
| Structured output | Typed contract tests and actual Inspector/client responses above | Satisfied |
| Timeouts | [M1-06 active cancellation/EOF](M1-06.md), [M1-09 quality cancellation](M1-09.md), full19 Rust security | Satisfied for candidate source; Inspector evidence alone proves only client notification |
| Filesystem restrictions | [M0-04 roots/no-follow/races](M0-04.md), M1-01/M1-07, M1-10 authenticated import boundaries | Satisfied for implemented adapters; **Windows reparse-safe adapter/native evidence absent** |
| MCP operates with network disabled | M1-09, full19 Docker/Rust/audit/semantic gates with positive controls and enforced deny | Satisfied in qualified macOS/Docker configuration; no native Linux/Windows claim |
| Offline snapshot import | [M1-10](M1-10.md) authenticated import, limits, extraction, activation, antirollback and recovery | Satisfied on macOS candidate source |
| SQLite lexical and LanceDB semantic search | [M1-12 native E5/Lance/FTS](M1-12.md), Inspector hybrid call, fresh full19 semantic/search stages | Satisfied on macOS ARM64 |
| Correct fallback when LanceDB/embeddings are absent or invalid | M1-11/M1-12 degraded/fallback tests and fresh full19 | Satisfied |
| Catalog provenance/freshness | M1-11..13 plus actual Inspector/client catalog status | Satisfied |
| Integration tests | [Full19 report/log/counts](M1-17-final-gate.md) | Satisfied on macOS ARM64; platform row still blocked |
| MCP Inspector tests | [Inspector 2.5.0 UI qualification](M1-17-inspector.md) | Satisfied for tools; UI Resource read remains explicitly unqualified and is not a DoD substitute |

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
| 12 | stay inside the host-authorized filesystem closure | M0-04 adversarial roots/no-follow/race tests; per-session trusted roots | Satisfied for macOS/approved Docker; native platform matrix blocked |
| 13 | cancel processes | [M1-06](M1-06.md) observes active children and joined kill-tree cleanup; Inspector sent cancellation but did not prove active child state | Satisfied by product gate, not by Inspector alone |
| 14 | get clear errors when an external tool is missing | Fresh exact-binary stock Codex run returned structured `SANDBOX_DENIED`; the exact candidate CLI returned `unavailable/InvalidConfiguration` for nonexistent Docker; M1-14 cases | Satisfied |
| 15 | search crates locally through SQLite FTS5 | [M1-12](M1-12.md), full19 and Inspector lexical call | Satisfied |
| 16 | search by intent through LanceDB | M1-12 actual E5/Lance and Inspector hybrid call | Satisfied on macOS ARM64 |
| 17 | know snapshot/freshness used | M1-11..13 and actual Inspector/client catalog status | Satisfied |
| 18 | operate with network completely disabled | M1-09/full19 enforced network-deny tests with positive controls | Satisfied for macOS host/approved Docker; native Linux/Windows evidence absent |

## Release blockers kept in separate categories

| Category | Open item | Why it remains open |
| --- | --- | --- |
| Resolved owner decision | Product license and copyright | ADR-047 selects `MIT OR Apache-2.0`; copyright IUMotion Labs; root license files apply to original code |
| Resolved owner decision | Source publisher/channel | IUMotion Labs via `pharos-lang/rust-engineering-mcp`; GitHub is the source channel and GitHub Releases the initial binary channel |
| Resolved delivery decision | GitHub artifact provenance | Keyless GitHub OIDC attestations; no long-lived repository signing secret |
| Owner decision | Production catalog signing-key custody, rotation and revocation | ADR-041 requires an explicit Ed25519 trust root; public fixture keys are forbidden for production |
| Evidence gap | Kanaria 0.2.0 license text | Exact recorded VCS commit/root and LICENSE return 404; manifest declaration is retained but no text is invented |
| Evidence gap | E5 model license-text packaging | Exact model revision declares MIT in its card but has no license-named file; redistribution disposition remains absent |
| Evidence gap | Static ORT/native attribution | Versioned ORT LICENSE and ThirdPartyNotices exist, but the actual static archive lacks a complete object/options-to-notice attribution |
| Evidence gap | Per-target final notice inventory | Current 587-package/991-text inventory and 30/31 upstream supplement are a local candidate superset, not a final bill per native artifact |
| Resolved product metadata | Eight workspace crates previously had no product grant | All inherit `MIT OR Apache-2.0`; release-specific third-party notice work remains open |

M1-15 and M1-17 stay **Blocked** until every remaining evidence gap and the catalog
key decision are resolved. A declaration, `cargo deny` success or a local review
copy is not redistribution approval.

## Additional release gates

| Gate | Evidence | Status |
| --- | --- | --- |
| Candidate binary/source link | [238-input equivalence receipt](../release/candidate/m1-17-source-equivalence.json) | Passed without rebuild; reproducible-build equality not claimed |
| Full local gate | [Fresh full19](M1-17-final-gate.md) | Passed all 19 stages on macOS ARM64 |
| Third-party clients | [Inspector](M1-17-inspector.md), [stock Codex](M1-17-codex-client.md) | Direct evidence passed; two bounded stock-model turns failed without product calls |
| Tool contract stability | [Two-session canonical inventory](m1-17-codex-client/tool-inventory-canonicalization.json) | Deep-equal; insertion-order historical digests superseded |
| Native semantics/performance | M1-12 actual paths; M1-16 benchmark and measured pilot | macOS ARM64 evidence only; no general utility claim |
| Licensing/notices | [Preparation](../release/preparation.md) | Product grant resolved; third-party/native/model evidence above remains blocked |
| Signed distribution | GitHub OIDC workflow plus development catalog receipts | Binary publication blocked by native/notices; catalog distribution blocked by its Ed25519 key decision |
| Independent final review | [Opus 5 read-only review](M1-17-review-opus.md) and [principal disposition](M1-17-review-disposition.md) | Completed; model-authored, not human; surviving blockers retained |
| Closure | This matrix and board | **Blocked**; source is public, M1 binary/catalog releases are not published |

## Native matrix and capability boundaries

| Platform | Observed status | Required before qualification |
| --- | --- | --- |
| macOS 26.6.2 ARM64/APFS | Source-bound installs, Inspector UI, stock client and [fresh full19](M1-17-final-gate.md) passed | Third-party notices, catalog key and final artifact decisions still block release |
| Linux ARM64 native | No native receipt | Native core/protocol/catalog/ORT/LanceDB and capability tests |
| Linux x86_64 native | No native receipt | Same plus target-specific model/runtime/build/install receipts |
| Windows x86_64/ARM64 | No native receipt or qualified protected-I/O adapter | Native gates and reparse-safe adapter before filesystem access is enabled |
| macOS x86_64 | No native receipt | Runner and native ORT/LanceDB/build/install evidence if advertised |
| Docker Linux ARM64 guest | Approved execution image and adversarial project-code tests | Never counted as native OS/library distribution qualification |

Spec §M1 names three OS families; ADR-029 additionally gates every advertised
architecture. Absence of a runner is pending evidence, never a passed skip. Any
scope reduction requires an explicit owner decision plus updated ADR/spec/status.
