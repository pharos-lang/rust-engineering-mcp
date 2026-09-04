# M1-06 principal disposition

2026-09-04; independent Opus5 High plus principal diff/source review.

- P1 forged build-finished: CONFIRMED by actual proc-macro stdout before E0308
  and genuine build-finished=false, exit101. Fixed bounded tail scan: any additional
  Cargo event makes phase incomplete and clears build_succeeded. Ordinary harness
  text remains unparsed. Unit discriminator plus actual Docker fixture; raw log
  preserved. No fixture compiled on host. This closes a real evidence-integrity bug.
- P2 cold budgets: containment timeout20s/observer18s; MCP timeout15s. Assertions
  still require actual test markers/processes, not merely generic timeout. Gates
  remain serial. No production timeout expansion beyond the documented1..60s.
- P2 collapsible else-if: core Clippy had passed, but simplified nesting as suggested.
- P2 custom harness: ADR, docs and tool explicitly explain fixed harness arguments
  may cause a custom harness to reject invocation and fail.
- P2 readOnlyHint: deliberate enforced host-effects parity. Source RO, ephemeral
  isolated writes, denied network; explicit R2 execution warning in tool/ADR.
- P2 log framing: human, unescaped, project-forgeable headings now documented. No
  parser or Resource authorization trusts section delimiters as producer identity.
- P2 fingerprint: rust_gateway.rs itself is included in implementation_fingerprint.
  Exact sealed-argv tests pin positional filter and terminal harness separator.
  No generic append follows TestProject construction; no new flag injection path.

No unresolved confirmed P0/P1 after the fix; final integrated gates remain required.

Follow-up Opus review: confirmed original P1 closed; proposed an interleaved malformed
tail residual. Hardened scan to reject any literal quoted reason marker as well as
valid events, with garbage-prefix, whitespace/unknown-reason tests. This is bounded
ambiguity detection, not authentication of arbitrary project-writable bytes.
The suggested assumption that complete diagnostics become compiler-authenticated
is rejected: completeness is parsing/capture coverage, never producer identity.
The suggestion to erase every reported phase on timeout/truncation is also rejected:
M1 explicitly retains safe partial evidence; build_succeeded is reported evidence
alongside validation_complete=false, never alone a success predicate. Added explicit
unit assertion for this partial-field contract. An incomplete failed-build tail
and benign Cargo-looking harness output are documented as conservative rejection.
No claim is made to detect every possible interleaving or fabricated diagnostic.
