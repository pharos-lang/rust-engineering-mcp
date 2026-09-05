# M1-17 principal disposition of independent review — historical candidate

The exact external result is preserved in
[m1-17-review/review.json](m1-17-review/review.json). The reviewer returned
`blocked`; that verdict is retained. This disposition resolves the correctable
P0/P1 evidence defects and keeps the substantive platform, license and release
blockers open. No production source or public contract changed.

This file records the state when that review was issued; it is not the current
closure verdict. ADR-048 later resolved the platform/artifact/catalog boundary but
did not retroactively change the reviewer output or qualify final release bytes.

Post-review follow-up: ADR-047 subsequently resolved the original-code license,
copyright, source publisher/channel and GitHub artifact provenance. It did not
resolve third-party notice gaps, native runners or the production Ed25519 catalog key.
ADR-048 subsequently excluded model/ORT/LanceDB/catalog/trust and additional native
artifacts from 0.1.0, selected macOS ARM64/APFS as the only positive host, and chose
not to publish an official catalog. Those historical gaps remain source-builder or
future-distribution limitations, not blockers for the excluded 0.1.0 bytes.

| Finding | Principal disposition | Evidence / remaining boundary |
| --- | --- | --- |
| P0: client binary bound to `01a90ab6`, candidate declared `d024c7c` | **Resolved.** | [Fresh equivalence receipt](../release/candidate/m1-17-source-equivalence.json) compares all 238 selected inputs against `d024c7c` and the working tree: zero mismatches. It does not claim a rebuild. |
| P1: four unstable `tools_sha256` values | **Resolved as a receipt defect.** | Historical hashes encoded insertion-order maps. Two fresh stock-client sessions returned deep-equal maps and canonical digest `7c83911d…`; thirteen per-tool hashes are in the [receipt](m1-17-codex-client/tool-inventory-canonicalization.json). Current controller names the field `canonical_tools_sha256`. |
| P1: matrix claims local completion while listing uncited requirements | **Resolved.** | The reviewed revision of the [matrix](M1-17-matrix.md) added Required evidence / Satisfying receipt / Status columns and recorded its then-current explicit Blocked state. The live matrix is now In progress under ADR-048. |
| P1: no §117 criterion traceability; repair/cancel/missing-tool gaps | **Resolved with qualification.** | The matrix now maps all 18 criteria. A fresh exact-binary stock-client [receipt](m1-17-codex-client/repair-missing-receipt.json) records E0502 → external edit → passing check and nonexistent Docker → structured `SANDBOX_DENIED`; the exact candidate capabilities CLI also returns `unavailable/InvalidConfiguration`. M1-06 supplies active-child cancellation and M1-16 supplies model-authored patches. Section 117 does not require one stock-model session; model-driven stock Codex remains unproven. |
| P1: license evidence gaps hidden as owner decisions | **Confirmed for the reviewed candidate; later scoped by ADR-048.** | Kanaria/E5/ORT/LanceDB are prohibited from the core archive. Their gaps remain documented for source builders; the final core closure still requires exact notices/SBOM. |
| P2: audit/deny lack advisory DB identity | **Resolved for the current claim; historical limitation retained.** | [Focused repetition](m1-17-final-gate/audit-focused.json) binds both exit-0 commands to Cargo.lock and RustSec commit `5a0ebed…`, including three old untracked placeholder advisories. No database-cleanliness or remote-freshness claim is made. |
| P2: inverted Codex configuration booleans | **Resolved as naming ambiguity.** | [Sidecar](m1-17-codex-client/effective-config-semantics.json) defines historical polarity; current controller uses `effective_feature_values` and `effective_host_server_enabled`. Its 11 tests pass. |
| P2: saturated M1-16 endpoint and unfavorable observed cost | **Confirmed limitation, now explicit.** | [Report](../research/m1-16/measurement/REPORT.md) states zero discriminating power/no equivalence and records B/A ratios: requests 2.421x, input tokens 4.226x, participant time 1.703x. No product-value claim is made. |
| P2: gate timestamps/counts unsupported | **Historical limitation retained; future path resolved.** | [Derived counts](m1-17-final-gate/counts-derived-from-log.json) report 644 workspace-test passes and one separate doctest. Exact start is not reconstructed. Gate report v2 records timestamps/counts directly for the final repetition. |
| P3: wrong gate source branch | **Resolved.** | Receipt now records `ai/m1-17-release-qualification` and notes that the same commit was also `main` tip. Post-gate documentation is identified as subsequent evidence. |
| P3: Inspector cancellation may precede active work | **Confirmed boundary.** | Inspector text continues to claim only a cancellation notification. Active-child/kill-tree evidence comes from M1-06/M1-14, not the UI event. |
| P3: inconsistent redaction and narrow secret scan | **Resolved for local retention and public export.** | [Follow-up scan](m1-17-final-gate/secret-scan-followup.json) covers seven credential classes and reports zero hits. ADR-047's history-free exporter replaces local home/user paths and records old/new hashes in `PUBLICATION-SNAPSHOT.json`; private originals remain local. |
| P3: `approve` on thirteen code-executing tools | **Accepted only for this harness.** | The exact plan/tool allowlist had separate principal approval and direct calls were fixed. The docs and [sidecar](m1-17-codex-client/effective-config-semantics.json) now state that this is not a recommended client posture. |

The independent-review requirement in `AGENTS.md` explicitly calls for Claude Code
as an external reviewer, so this run satisfies that repository gate. It remains
model-authored and is not represented as a human review.

No P0 remains. The matrix-accuracy and tool-contract P1 defects are resolved. The
license-evidence P1 and platform/key questions blocked that reviewed candidate.
ADR-048 later resolved their applicability to 0.1.0. M1-15/M1-17 are now **In
progress**, not Done, pending new final artifact/full/client/review/publication
evidence. A new final review and disposition must assess the final candidate rather
than editing this historical result.
