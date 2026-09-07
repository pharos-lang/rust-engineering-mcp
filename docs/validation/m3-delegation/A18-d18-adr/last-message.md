I've completed A18. Here's the delivery summary.

## Task
A18 (D18 prerequisite of M3-03/M3-04): write `docs/adr/ADR-062-coverage-accounting-and-semver-baselines.md` as **Proposed**, deciding LLVM coverage accounting and authorized SemVer baseline contracts, per the package brief.

## Result
Wrote ADR-062 deciding: coverage plugin/LLVM identity capture and calibration seam (extending the existing closed `RustCommand`/`RustGateway` pattern with two argument-free version-probe commands); two-phase `--no-report`+`report` capture as the only valid way to derive JSON/LCOV/HTML from one run; full (non-`--summary-only`) JSON as authoritative with explicit denominators/exclusions/cfg/doctest recording; a domain-level "zero data is not 100%" contract rule; multi-package merge rules with shared-file dedupe; branch-name-independent baseline identity; a `ToolStatus`-based coverage outcome taxonomy; a SemVer two-`ProjectRef` input contract (stable lock order, distinct snapshot identity, `--baseline-root` only, no URL/version/Git-ref, git-auto-detection neutralization); a provisional (explicitly unverified) exit-code table plus a concrete in-guest calibration protocol; SemVer report field shape; and required fixture families for M3-03/M3-04 with a positive control each. Confirmed ADR-060/061/062 were all free before writing (062 was picked per assignment; 061 was claimed concurrently by another worker during this session, no conflict).

## Files changed
- `docs/adr/ADR-062-coverage-accounting-and-semver-baselines.md` (new, 530 lines)
  SHA-256: `d32f7917ad8075a69845eeb2cb4ac14860ed6d4077042b1cb04f3f7a9c358cf2`

No other file was created, edited, or staged. `docs/adr/README.md` was not touched.

## Tests executed
None — this package is documentation-only (an ADR), per its own scope; no code was added or modified. Verification performed instead:
- `ls docs/adr | grep -E "ADR-060|ADR-061|ADR-062"` → exit 0, confirmed 060/061/062 free before writing (061 was claimed by a concurrent worker mid-session; disjoint from my file).
- `git status --porcelain` → exit 0, confirms only my ADR file plus other workers' disjoint untracked files exist; nothing of mine touched outside `docs/adr/ADR-062-*.md`.
- `shasum -a 256 docs/adr/ADR-062-coverage-accounting-and-semver-baselines.md` → exit 0, hash above.

## Evidence
Read in full before deciding: `sources/index.txt`, `llvmcov-readme.txt`, `semver-readme.txt`, `releases-summary.json`, `rust-llvm-tools-sha.txt`, `README-sources.txt` (orchestrator-fetched, all HTTP 200 relevant to D18); repo code `crates/domain/src/{rust_execution,check,test_run,inspection,execution,value,result,quality,evidence,resolution,artifact}.rs`, `crates/application/src/{quality,test_run,source,inspection}.rs`, `crates/execution-adapter/src/rust_gateway.rs` (full), `crates/execution-adapter/src/rust_calibration.rs` (partial, calibration pattern); `docs/tools.md` (rust.test/rust.quality.gate sections); ADR-012/028/030/031/033/037/040 (full); `docs/roadmap/m3-quality.md` (full) and `docs/roadmap/adr-backlog-m2-m8.md` D16–D19 region; spec §26 and §104/105.

## Risks
- The exit-code table (§9) is an unverified hypothesis from documentation only — flagged explicitly in the ADR text and Open issues, per the task's explicit "do not copy numbers" instruction.
- Artifact size ceilings (8 MiB HTML) are proposed placeholders, not measurements.
- Neither plugin is provisioned in the approved image; this ADR does not authorize that, and M3-03/M3-04 cannot execute anything real until a separate provisioning ADR lands.

## Decisions
All captured in ADR-062 §1–13 (identity/calibration seam, two-phase capture, authoritative JSON, zero-denominator rule, LCOV/HTML-as-artifacts, merge rules, baseline identity, outcome taxonomies for both tools, SemVer input contract, calibration protocol, report fields, fixture families) and the Alternatives-considered section (six rejected alternatives with reasons).

## Open issues
Listed in full in the ADR's own Open issues section: plugin provisioning/image update, unmeasured exit-code calibration, unmeasured artifact size ceilings, `ArtifactMetadata` kind/MIME gap (D17 dependency), undecided fail-under threshold semantics, unspecified git-auto-detection neutralization mechanism, undecided doctest-coverage scope, and out-of-scope coverage-`Failed`/cross-baseline-comparison questions.