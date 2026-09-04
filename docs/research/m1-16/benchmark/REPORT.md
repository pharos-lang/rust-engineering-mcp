# M1-16 native retrieval: bounded descriptive result

One authorized run completed: 8 queries × 3 modes × 3 warm repetitions,
plus one initial catalog.status and three excluded warmup searches (76 calls).
No participant-model calls, Docker operations, acquisition, or source changes.
This is native E5/Lance retrieval through the real MCP release and rmcp client.

| Mode | Warm requests | Median latency ms | Hit@5 | MRR@5 | nDCG@5 |
|---|---:|---:|---:|---:|---:|
| lexical | 24 | 0.476 | 0.125 | 0.125 | 0.125 |
| semantic | 24 | 4.040 | 1.000 | 0.9375 | 0.953866 |
| hybrid | 24 | 4.041 | 1.000 | 0.9375 | 0.953866 |

Relevance means only the preassigned identities in the closed 15-crate/16-version
projection. All three repetitions returned identical rankings. Only S02-en had
lexical results; the remaining seven multi-term literal queries returned none.
Semantic and hybrid found the expected identity first for seven queries and second
for S01-es. This reflects the exact authored queries, English annotations and
literal-token lexical contract; do not infer general IR superiority, multilingual
coverage, unbiased relevance, statistical significance, or agent utility.

Initialize/ready took 28.018 ms; discovery 13.578 ms; the first session catalog
load took 624.533 ms. This is not a cold OS page-cache measurement: import/rebuild
and artifact hashing had already accessed files. Warm query timings include SDK,
transport and JSON handling, and were collected while other host work could run.
The fixed serial order is disclosed in README and receipt; there is one cold load,
not enough to estimate its variance.

Peak observed MCP server RSS was 1,641,632 KiB across 22 samples (zero sampling
errors). Sampling used ps approximately every 50 ms plus command overhead; this
is not an exact OS peak, allocation profile or per-query memory increment. Fast
queries can have zero samples. The driver and other processes are excluded.

All measured requests retained their requested mode without fallback; snapshot
identity remained `sha256:28106d773b9efe8b7bba2f4908559ce305b4637b78e1c0e62c21ec5443b08728`
and matches the emitter's baseline projection. Post-run analysis checked all 243
returned rows against requested MSRV/non-yanked/non-prerelease filters. Model and
index availability/evidence are retained in the cold response. The labeled asset
hashes did not change, including five E5 files, authenticated bundles, baseline,
ORT build archive, executable and driver sources. Archive identity is not a full
linked-object provenance audit.

Loopback IPv4 and IPv6 controls connected outside the driver's network profile
and failed with EPERM inside it. No Internet endpoint was probed. The driver
reported ordinary joined shutdown, exit 0, server_joined true, no forced stop,
no cleanup error and zero stderr bytes.

Evidence: `run-01/receipt.json`, 76 complete numbered tool results,
`run-01/rss-samples.json`, `run-01/analysis.json`, `network-control.json`.
Receipt SHA-256: `28a846bc2b76f68e621d8a19b770aa7e3facb63dc4df1d2a07daec4dc688602d`.
The preparation check initially found that native-build-inputs lists the ORT link
search path rather than an archive row; the identity manifest uses the archive
at that recorded path. No benchmark was run until this was resolved. The only
native benchmark execution completed; no failed or selective rerun was discarded.
