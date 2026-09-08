# Technical Owner — final M4 closure disposition

Date: 2026-09-08. Verdict: **M4 Done locally** on `ai/m4-security`, base/HEAD
`c66a3704e1ad290603a3c1d10413df90d15c2b03`, working tree uncommitted, workspace
`0.3.0-dev`. The [independent Opus 5 High confirmation](review.md) accepts local
closure with no P0/P1/P2. Its original response and 59-file frozen package are
preserved. The retained stale-native P2 is closed by fresh scanner/Miri receipts;
the fingerprint-input P2 remains closed and is now bound to those receipts.

[Final verification](../../validation/M4-final-verification.json) independently
computes all 987 input comparisons, native/configuration hashes, 19 raw native log
hashes, the 23 Git baseline snapshot comparisons, release binary hash, exact E5
asset matches and empty owned Docker objects. Its [driver](../../validation/M4-final-verification-driver.py)
is retained. Static review and actual execution remain distinct evidence.

| Observation | Final disposition |
| --- | --- |
| P3-1 resume driver restates selected preflights | Accepted one-off recovery limit. Same toolchain/source and exact assets verified; only six non-audit remaining steps reused the original runner. A reusable resume feature should share the gate preflight and reject optimized Python; no general resume API is added in M4. Owner: Technical Owner, future separately scoped change. |
| P3-2 two-segment full timeline | Resolved by explicit `full_segments` in final verification: steps 1–27 retained and 28–33 executed, with timestamps and original receipt identity. Original full receipt remains unchanged. |
| P3-3 gate logs omitted from frozen package | Both reads made by the reviewer outside the package are disclosed and preserved with hashes in [additional inputs](additional-inputs.json). [Log archive](../../validation/M4-log-archive.json) retains 38 byte-identical, Git-visible copies; future review packages must include their raw gate logs. |
| P3-4 snapshot baseline missing from package | Final verification compares each prior snapshot to actual `git show HEAD:path`; all 23 are equal. Exact baseline bytes and hashes are retained under `additional-inputs/git-baseline/`. These supplemental checks are by the Technical Owner, not represented as newly reviewed by Opus. |
| P3-5 E5 expected pins not in recovery receipt | Final verification includes all five expected and observed sizes/hashes, explicit matches and the product pin-source digest; all match. Original recovery receipt unchanged. |
| P3-6 native receipt lacks post-run unchanged flag | Actual runtime harness checks all source/config hashes after execution and fails on drift; the full step exited 0. Final verification also recomputes every hash against the current files and records explicit equality; script attached as supplemental evidence. |
| P3-7 inventory canonicalisation unspecified | Final verification states exact UTF-8 JSON canonicalisation and recomputes the index digest; all 987 entries also compared elementwise. |
| P3-8 accepted inference/privacy limits and prior P3-A | Accepted and documented: raw Codex cleanup object not retained, filename-shaped guard, authenticated JUnit classification boundary, incomplete syntax scan and authorized-source HTML/diff content. A behavior-based fingerprint omission guard remains Technical Owner backlog; no mirrored literal-list test is substituted for native evidence. |

The reviewer disclosed three file reads and directory listings outside the frozen
package. The file bytes are recorded in `additional-inputs.json`; no write,
command or agent tool was used and the CLI reported zero permission denials.
Substantive review used Opus 5; CLI usage also includes 19 auxiliary Haiku output
tokens, preserved in the final verification metadata, not a substituted review.

After package freeze, documentation-only edits added M4 reproduction commands to
CI, linked archived logs, removed a duplicate M4 inventory changelog bullet under
historical M1, linked final dispositions and changed milestone status to Done.
No qualified source/config/fixture input changed. The roadmap planning baseline
is not retroedited. Existing M3 debts and the two unreproduced qualification
failures remain disclosed; no unobserved root cause or causal fix is claimed.

All six cuts and G1–G9 are satisfied for the declared local target. Core 19/19,
full 33/33 (27 retained + six resumed), M4 runtime 19/19 and final client attempt 4
pass. Remote CI/Sonar and Linux/x86_64 host qualification are not credited.
This decision authorizes no commit, PR, tag, release, publication or M5.
