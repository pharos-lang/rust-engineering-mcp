# M1-16 measured utility pilot

Date: 2026-09-04. Freeze SHA-256:
`80a751b391f49460b555e8fabd71f8c68f593bf0f2a3fba0341357d02a1478c6`.
The prospective v2 schedule completed all 24 planned runs with
`gpt-5.6-sol`, medium reasoning, no model fallback and one repetition per item.
All participant, cleanup, catalog-identity and post-oracle freeze checks were
verified. This is a small descriptive feasibility pilot over four std-only repair
tasks and four crate-selection intents in English and Spanish.

## Result

| Outcome | Arm A | Arm B |
| --- | ---: | ---: |
| First candidate passed | 12/12 | 12/12 |
| Final candidate passed | 12/12 | 12/12 |
| Candidate revisions | 0 | 0 |
| Participant/infrastructure failures | 0 | 0 |

All 12 pairs were `both_pass` at first and final scoring. There are no discordant
pairs from which to estimate a success difference. The observed evidence therefore
does not support the repair-success or iteration-reduction advantage hypothesized
for B in this corpus. It does establish that the complete B workflow can produce
reviewed passing repairs and source-grounded offline selections through the real
M1 MCP surface.

The primary endpoint is saturated: 24/24 first candidates passed. This ceiling
effect gives the pilot zero discriminating power for success-rate or repair-loop
differences and is not evidence that the two interfaces are equivalent. In the
only directional observations, B used 2.421 times the tool requests (92/38),
4.226 times the input tokens (751,759/177,868), and 1.703 times the participant
elapsed time (585.143/343.500). These descriptive ratios do not establish a
population cost effect, but they also cannot support a product-value claim.

## Interaction and timing observations

| Observed total, 12 runs per arm | Arm A | Arm B |
| --- | ---: | ---: |
| Candidate submissions | 12 | 12 |
| Validation cycles | 4 | 8 |
| Tool requests | 38 | 92 |
| Participant elapsed | 343.500 s | 585.143 s |
| Total elapsed | 355.486 s | 595.978 s |
| Catalog setup | 9.691 s | 7.645 s |
| Cleanup | 1.787 s | 2.349 s |
| Input tokens | 177,868 | 751,759 |
| Cached input subset | 72,192 | 499,584 |
| Output tokens | 7,250 | 13,342 |
| Reasoning output subset | 1,893 | 3,932 |

Usage was reported for all 24 runs. Cached input is included within input and
reasoning output within output; neither subset is added again. B made 27 file
reads, 9 crate searches and 20 crate inspections. A made 14 file reads and 8
plain projection reads. The nine B searches retained their requested and effective
modes: eight hybrid and one lexical, with no fallback. No Resource read occurred
inside the utility runs; Resource behavior was qualified separately before freeze.

These totals describe this fixed execution only. Pair order, provider caching,
Docker image/OS caches and different interface representations can affect token
and elapsed totals. The study was not powered for inference, has no within-item
replication and does not establish that either arm is generally faster or cheaper.

## Oracle and review evidence

Every repair candidate passed the hidden `fmt`, `check`, strict Clippy and test
stages: 8/8 at each stage. All eight also passed the identical MCP
`rust.dependencies.audit` closure. All 16 selection candidates passed the frozen
identity, version, MSRV, license, corpus-date, snapshot-fingerprint and provenance
predicate. First and final hashes were identical within every run.

The independent masked Sonnet 5 review covered 20 of 21 exact candidate hashes and
reported no patch defect. It emitted four P3 observations questioning provenance
details. The principal preserved those findings and did not confirm them because
the frozen authoritative projection explicitly records both
`source_kind=registry_snapshot` and `network_used=false`. The external JSON also
duplicated C008's hash in row C007, omitting C007's expected hash; the principal
reviewed that selection directly against its candidate text and frozen projection.
The complete external output, private mapping and principal disposition are
retained without rewriting the reviewer result.

## Security and scope observations

No run emitted an unadmitted request or retryable broker denial. Before and after
observations found no owned Docker container or volume residue, and all process,
SDK and gateway joins completed. Each arm passed the exact pre-model catalog,
model and semantic-index identity check. The frozen inputs were verified after
every participant and every oracle.

This pilot does not qualify native Linux, Windows or x86 targets, product/model
redistribution, publisher trust roots, stock-client configuration or the earlier
96-run proposal. The selection snapshot has 15 crates and is an authored research
fixture; the RustSec input has one record and the repair projects have no
third-party dependencies. No general safety, registry coverage or causal utility
claim follows.

## Retained artifacts

`results/raw/` contains the 24 participant receipts, event streams, candidates,
workspace snapshots and oracle receipts. `results/analysis/` contains the
machine-readable aggregate, source links and descriptive summary.
`results/review/` contains the masked packet, raw reviewer response, mapping and
principal review. `results/SHA256SUMS.json` records SHA-256 and size for all 417
retained result files; the secret-pattern scan was empty before preservation.
