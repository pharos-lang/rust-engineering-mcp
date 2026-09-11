# Technical Owner disposition — final code review

The original Opus 5 High response is preserved. No P0/P1 or new P2 was found. This historical code-review disposition is completed by the [final evidence acceptance](../m4-final-evidence/review.md) and [Technical Owner closure](../m4-final-evidence/disposition.md).

| Finding | Disposition |
| --- | --- |
| Prior P2-2 fingerprint input set | Closed in source by Opus. Final native receipts must still bind those bytes. |
| Retained P2-1 scanner/Miri native receipts | Final full regenerated seven scanner, thirteen Miri classification and seven admission cases on current sources; original stale receipts preserved under native-before-final. Final evidence confirmation accepts and closes this P2. |
| P3-A regression guard for fingerprint set | Technical Owner backlog: consider a behavior-based mutation guard for omitted parser/port inputs. Do not replace the existing native source-bound evidence with a test that only duplicates the literal implementation list. |
| P3-B benchmark and sync authorization provenance | The source inventory exists in `docs/validation/M4/budgets/m4-budgets-inputs.json`, paired with the exact frozen binary SHA. The review package omitted this supporting attachment. Final evidence confirmation includes it and a delta inventory. The benchmark remains explicitly historical for the final routing/parser changes; successful current client flows qualify those separately. The final client execution receipt links the exact budget receipt used for its explicit environment gate. |
| P3-C closure docs lag focused receipts | Updated after final source-bound execution and independent evidence acceptance. |
| P3-D credential guard is filename-shaped | Accepted limit. The real boundary keeps client auth outside the repository, with metadata-only wire observations. The staging guard is not advertised as universal content secret detection. |
| P3-E JUnit-present classification ignores outer process streams | Accepted ADR-072 boundary. Muted guest output and bounded authenticated runner evidence determine classification; project prose is not trusted as diagnostic truth. |

After review, core discovered an outdated corpus SHA and an intermittent stdout observer assertion. The corpus SHA now matches the explicitly added cloud-metadata denial fixture. The test observer now reports variant/time without accepting errors or extending its deadline; 30 isolated and five complete protocol repetitions passed. These changes are qualification metadata/diagnostics only; the final confirmation must inspect them and their retained failure evidence. No product security decision was changed.

Final core 19/19, full 33/33 (27 retained + 6 resumed on identical sources after exact local E5 asset recovery), native 19/19 and client attempt 4 passed. Full attempt 1 also retained an unreproduced legacy qualifier cleanup failure; ten isolated repetitions and the final full qualifier suite passed without oracle changes. The evidence-only confirmation inspected and accepted these dispositions; no unobserved root cause is claimed.
