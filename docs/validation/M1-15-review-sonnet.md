# M1-15 independent review

Claude Code2.1.259, explicit claude-sonnet-5/medium, tools disabled, strict empty MCP, no file changes. The receipt also reports a separate auxiliary Haiku4.5 call by the CLI; substantive review is Sonnet5.

Packet SHA256: `fcd31e628e329f9280dd52469fc852d7b5bfba3d0147e96c2c742b10394afd31`. Result SHA256: `6e93581ebfd35d672ffd685ceb98862f35a66ffa0ecfad6baa5df5571ae28ac6`.

Reviewed the M1-15 offline-candidate doc, its four reproduction scripts, and the checked-in candidate receipts. This is a well-built, appropriately-scoped local trusted-input harness. No High/Critical issues found. Findings below by severity.

## Info / good practices confirmed (not issues)
- **Input integrity gates before use**: `build-candidates.py:48` rehashes the ORT archive (exact filename set + SHA256) before allowing the build to proceed; `verify-installation.py:150-153` rehashes all five E5 model files against `model-receipt.json` before copying. Untrusted/mismatched local bytes cannot silently enter the candidate.
- **Symlink rejection on all trusted copies**: `verify-installation.py:36-37` (`copy()`) and `build-candidates.py` refuse anything that isn't a regular file, closing an obvious local substitution vector even though the threat model is "owned directory, not hostile local writer."
- **Linkage allowlist is enforced, not just logged**: `verify-installation.py:73-74` raises if any Mach-O dependency isn't under `/usr/lib/` or `/System/Library/Frameworks/`, which is exactly what the doc claims ("fail on unresolved build-tree/private-cache dylib paths").
- **ORT-symbol/feature-consistency check**: `verify-installation.py:109-111` asserts `_OrtGetApiBase` presence correlates exactly with `feature == 'local'`, catching a mis-wired build rather than trusting the cargo feature flag alone.
- **No silent overclaiming from passive doctor**: `verify-installation.py:183-184` asserts passive mode never reports `runtime` populated or a non-`warning` status; lines 198-210 hash-compare `active.bundle`/`floor.record` and a sentinel file before/after passive doctor to prove it took no admin lease and didn't touch staging.
- **Diagnostic commands don't auto-repair**: `verify-installation.py:224` asserts a missing-catalog doctor call neither creates the missing directory nor exits 0 — a real regression guard against a diagnostic becoming a mutator.
- **Archive integrity without extraction**: `package-candidates.py:54-66` streams and hashes every tar member via `tar.extractfile()` without ever writing archive contents to disk, checks for duplicate/unexpected member names, and confirms full membership — this avoids path-traversal/zip-slip risk entirely since nothing is extracted to a filesystem path.
- **Isolated Docker config for active verification**: `verify-active.py:12` writes a fresh empty `config.json` rather than reusing the operator's Docker credential store, avoiding incidental credential exposure during automated test invocations.
- **Product importer used as-is**: `verify-installation.py:191-193` drives the real `catalog import` CLI rather than a bespoke bundle loader, so the harness doesn't create a parallel/weaker trust path around the actual catalog trust boundary. Consistent with the constraint not to invent a generic installer.

## Low
- **`build-receipt.json`'s `source_commit`/`source_status` fields are misleading in isolation.** `docs/release/candidate/build-receipt.json:4` records `source_commit: "3bb9b8b3..."` (a pre-M1-14 commit) with a large `?? .../M1-14...` dirty-status list, even though the final accepted candidate's source content was later confirmed to match merged `main@01a90ab6` via `accepted-source-receipt.json`. Anyone reading `build-receipt.json` alone would misattribute the candidate to the wrong HEAD. The doc text (`offline-candidates.md:12`) and `accepted-source-receipt.json` correctly layer on the authoritative comparison, so this isn't a defect — but it's worth calling out explicitly as a "read both receipts together" caveat rather than something self-evident from `build-receipt.json` in isolation.
- **Dense boolean expressions rely on Python operator precedence without parentheses**: `build-candidates.py`'s `sources()` filter (`path in (...) or path.startswith(...) and (...)`) and `package-candidates.py`'s per-feature file filter (`row['path'].startswith(feature+'/') or row['path'].startswith('assets/') and (...)`) both evaluate correctly (verified against the observed archive payload sizes — core 28MB excludes the model, local 766MB includes it), but the lack of explicit grouping makes correctness depend on remembering `and` binds tighter than `or`. Purely a readability/maintenance risk, not a current correctness bug.

## Not flagging (explicitly out of scope per your framing)
- No "generic hostile installer" hardening was demanded or is missing here — this is a developer-owned reproduction harness for trusted local bytes, not the product's importer, and it correctly treats the real catalog CLI as the actual trust boundary.
- Missing/pending license texts (Kanaria, E5 packaging, product LICENSE) are already tracked as open issues in `native-license-evidence.json:215-219` and repeatedly marked `owner_decision_pending` rather than being silently glossed over — that's the correct posture for local review copies, not a gap to re-report.

**Summary**: no security or correctness defects in the harness logic itself; one low-severity provenance-labeling caveat and one readability nit, both non-blocking.
