# M1-08 principal disposition

Sonnet5 Medium read-only review found no P0. Its proposed P1 is a hypothetical
compatibility risk, not a reproduced defect: unexpected stderr could block an
otherwise successful explanation. The fixed approved1.98.1 image, clean environment
and literal rustc argv are deliberate; unexplained stderr/exit is Infrastructure,
not silently accepted. Actual E0502 success and E9999 absence passed including
exact stderr classification. No claim covers every diagnostic code; unknown/partial
outputs fail closed. No speculative broad scan or permissive parser was added.

- Freshness60/300 is conservative age of the captured installed-runtime observation,
  not a claim that immutable explanatory content changes after300seconds. Returned
  assessment is a snapshot at assessed_at; it does not mutate itself later. SHA and
  immutable runtime identity independently identify the content/compiler.
- MCP classification guards are intentional independent contract defenses. The
  packet omitted tests; explaining/tests.rs already exercises truncated/timeout/
  oversized/inconsistent Ok observations. Removing guards would weaken the boundary.
- Exact unknown-code vocabulary is explicitly version scoped. Current real runtime
  test and configuration/calibration fingerprint must be rerun whenever approved
  image/toolchain changes. No runtime bump is silently accepted.

Validation: core474/10stages; actual no-project MCP10cases; calibration six scenarios.
Diff reviewed, fmt/clippy/contract/protocol/architecture and dependency gates passed.
Native platforms and full release qualifications remain unchanged. No new blocker.
