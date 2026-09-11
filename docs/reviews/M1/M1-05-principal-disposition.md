# M1-05 principal disposition

2026-09-04. Sonnet5 Medium independent review plus principal diff/source review.

- P1 code-less root labels: normalization convention explicitly documented, not
  authenticated producer inference. clippy:: roots and descendants are clippy;
  other roots retain historical rustc label, including code-less messages. No
  diagnostic is dropped/made passed by that label. Extended discriminator tests
  retain a code-less top-level diagnostic with null code and full completeness.
  No verified pinned Clippy lint missing its lint code was supplied by review;
  don't invent a producer identity from absence of code or change all rustc errors
  in a Clippy invocation to clippy. This remains a declared labeling limitation.
- P1 calibration nondefault path: review misidentified configuration_fingerprint's
  serialization table as startup execution. Calibration remains six actual
  containment scenarios; new explicit Clippy build.rs/proc-macro cases test its
  execution path. Actual MCP cases additionally exercise strict/pedantic fixed
  separator/args. Applied argv/env verification covers the actual selection each
  execution; no shell splitting or arbitrary flags. Same sandbox enforcement.
- P1 budget: availability choice, not security defect. Check also executes build.rs
  and proc macros. Both keep30s and256KiB per stream; Clippy adverse and normal
  cases must pass final real gate. Larger jobs explicitly time out/incomplete;
  no claim of general project throughput or benchmark utility.
- P2 repeated argv projection: bounded four-field construction, distinct validated
  types and exact argv tests. No shared polymorphic flag layer added for these
  few fixed fields; both validate feature grammar/sorting through CheckOptions.
- P2 trim test gap: supplied packet omitted tests, code already has dedicated
  clippy/tests.rs::response_budget_drops_diagnostics_but_keeps_log_and_downgrades_passed
  with128 escaped large findings. It verifies total limit, omissions, downgrade,
  schema and retained log. Not missing implementation evidence.
- Default/project alias is intentional and tested/documented.

No confirmed unresolved P0/P1 product defect. Live tests and final gates remain
mandatory; static review alone is not containment qualification. No reviewer
suggestion changed scope, runtime, sandbox policy or established public tools.
