# M1-16 descriptive pilot analysis

Candidate success, participant completion and infrastructure status remain separate. Unknown outcomes and missing usage are not imputed. This report makes no causal or population claims.

| Arm / family / language | Planned | Recorded | Infra failed | First pass / evaluated | Final pass / evaluated | Final unknown |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| A / repair / en | 4 | 4 | 0 | 4 / 4 | 4 / 4 | 0 |
| A / selection / en | 4 | 4 | 0 | 4 / 4 | 4 / 4 | 0 |
| A / selection / es | 4 | 4 | 0 | 4 / 4 | 4 / 4 | 0 |
| B / repair / en | 4 | 4 | 0 | 4 / 4 | 4 / 4 | 0 |
| B / selection / en | 4 | 4 | 0 | 4 / 4 | 4 / 4 | 0 |
| B / selection / es | 4 | 4 | 0 | 4 / 4 | 4 / 4 | 0 |

| Pair outcome | First | Final |
| --- | ---: | ---: |
| both_pass | 12 | 12 |
| both_fail | 0 | 0 |
| A_only | 0 | 0 |
| B_only | 0 | 0 |
| unknown | 0 | 0 |

| Arm | Candidates observed | Validation cycles observed | Tool requests observed | Elapsed seconds observed | Input tokens observed | Cached input subset | Output tokens observed | Reasoning output subset |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| A | 12 (n=12) | 4 (n=12) | 38 (n=12) | 355.486 (n=12) | 177868 (n=12) | 72192 (n=12) | 7250 (n=12) | 1893 (n=12) |
| B | 12 (n=12) | 8 (n=12) | 92 (n=12) | 595.978 (n=12) | 751759 (n=12) | 499584 (n=12) | 13342 (n=12) | 3932 (n=12) |

Counts use all24 planned runs; success fractions use only explicitly evaluated outcomes. Token/timing totals show their observed-run coverage; cached/reasoning subsets are not additional tokens. Validation cycles and extra submissions are different operational counts. Pair order and warm caches can affect time.

Passing final candidates with failed infrastructure: 0. Artifact read issues: 0; per-run issues: 0.

Per-run task/turn/infra states, read counts, usage coverage, first/final references, oracle stages, raw timing observations and search fallback extracts are retained in [analysis.json](analysis.json). Every original artifact is linked with SHA-256 in [sources.md](sources.md).
