# M1 closure cut 1 — Claude Sonnet 5 review

Date: 2026-09-04. Review is read-only and precedes integration.

## Invocation receipt

- CLI: Claude Code `2.1.260`.
- Model requested and used: `claude-sonnet-5`.
- Effort: high.
- Session: `554808b3-ab36-41ec-8cd6-67ef51cc9a73`.
- Provider receipt UUID: `70306653-0f57-4f36-991c-58e0568ecd7a`.
- Mode: `--print`, `--tools ""`, `--strict-mcp-config`, permission mode
  `dontAsk`, permission prompts `none`, no session persistence, JSON output.
- `modelUsage` reported only canonical model `claude-sonnet-5`; no web requests,
  no tool calls, no subagents and no permission denials.
- Usage: 20,592 cache-creation input tokens, 3,289 cache-read input tokens and
  22,219 output tokens, including 20,262 thinking tokens.

The supplied packet contained the complete pre-review diff for ADR-048, the spec,
gate reporting, vendor verification and the new closure matrix. The concurrent
release-artifact scripts were explicitly not part of the reviewed cut.

## Verbatim finding set

The reviewer returned `REJECT` with these findings:

1. **P1 — Unverifiable vendor license claim.** The new LanceDB license hash was
   not accompanied in the review packet by the file contents or its upstream
   URL/revision and reviewer trail.
2. **P2 — Tool count is asserted, not reconciled.** The matrix did not print the
   literal thirteen-name inventory; its milestone rows name twelve MCP tools plus
   two CLI surfaces.
3. **P2 — Silent undercount risk in test-count parsing.** A passing test stage
   could emit no recognized summaries and still pass.
4. **P2 — New reporting fields lack an integration test.** Unit parsing tests did
   not exercise `run`/receipt wiring for timestamps and counts.
5. **P3 — Stream merge changes console behavior.** stdout/stderr are deliberately
   merged for a single chronological evidence stream; downstream consumers must
   not treat that stream as channel-preserving.
6. **P3 — Release artifact scripts are not yet wired.** Honest pending item, not a
   defect in this cut, but required before the tag.

The reviewer found no M2 scope and no silent security weakening, but could not
independently verify the thirteen-tool claim from the supplied packet.

## Principal disposition

- P1 fixed: `scripts/verify-vendor.py` now points to the retained upstream receipt;
  that receipt records exact Cargo/VCS revision, final URL, Git blob verification
  and the same SHA-256. The tracked license bytes are part of the next packet.
- Tool P2 fixed: the closure matrix now enumerates all thirteen public names.
- Count P2 fixed: test/doctest/gate-reporting stages require at least one recognized
  summary and become failed evidence when the expected summary is absent.
- Integration-test P2 fixed: focused tests exercise `run_step`, written fields,
  direct counts and the missing-summary failure path.
- Stream P3 accepted: merging is intentional so the bounded report records one
  chronological stream and counts both channels. No claim of channel separation is
  made.
- Artifact P3 remains pending until its separate cut is integrated and reviewed.

Post-fix evidence: `python3 -B -W error::ResourceWarning
scripts/test-gate-reporting.py` passed 6 tests; `scripts/verify-vendor.py` passed;
`git diff --check` passed. A same-model follow-up is required before integration.

## Follow-up 1 receipt and disposition

- Session: `837c12cb-f286-4c7e-8290-7ee6bdc8e5ec`.
- Provider receipt UUID: `7ad34b45-d010-4c51-b4a1-8d0cbca25ab1`.
- Model requested/used: `claude-sonnet-5`; effort medium; same closed invocation
  flags; no tools, web, subagents or permission denials.
- Usage: 9,429 cache-creation input, 8,413 cache-read input and 6,312 output tokens,
  including 5,024 thinking tokens.

The reviewer confirmed the hash/URL/revision/Git-blob chain and the missing-summary
failure logic, but returned `REJECT` for two packet-construction omissions: because
the matrix and focused test were untracked, `git diff -- <path>` supplied no
contents. It also retained the license applicability question because the upstream
receipt intentionally labeled that mapping as requiring owner review.

Principal disposition: the next packet supplies both untracked files with `sed`,
not `git diff`. ADR-048 now records the Technical Owner's explicit source-
distribution decision from the exact VCS manifest declaration, monorepo-root
Apache-2.0 bytes, verified Git blob and absence of a narrower subtree license.
LanceDB remains absent from the core artifact dependency closure. The spinner/
carriage-return rendering nuance of the merged chronological stream is accepted and
does not affect persisted byte counts or pass/fail semantics.

## Follow-up 2 final verdict

- Session: `51688922-c7da-4b5d-b4e9-bfa0b56eb3f4`.
- Provider receipt UUID: `2158ba60-0160-4a49-a2bc-5e9918119a29`.
- Model requested/used: `claude-sonnet-5`; effort medium; same closed invocation;
  no tools, web, subagents or permission denials.
- Usage: 18,565 cache-creation input, 8,413 cache-read input and 4,374 output
  tokens, including 2,980 thinking tokens.

The complete packet was **ACCEPTED**. The reviewer verified the four original
P1/P2 fixes, counted the literal thirteen tools, confirmed absence of
`rust.dependencies.inspect`, accepted the explicit owner risk decision and found no
remaining P0–P2. The only observation was that the ADR's Technical Owner acceptance
is self-attested rather than independently signed; this is not a closure blocker and
the current session is the authorized Technical Owner context.
