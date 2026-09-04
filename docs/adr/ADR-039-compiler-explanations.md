# ADR-039 — compiler-backed diagnostic explanations

## Status
Accepted for implementation 2026-09-04; current M1-08 gates required.

## Context
Spec23.10 requires rust.diagnostics.explain using installed rustc evidence, not a
heuristic borrow-check helper. No project_ref is present in the input contract.
The only approved compiler execution boundary is the calibrated Docker Rust gateway.

## Decision
Accept exactly code E followed by four ASCII digits, represented by a validated
DiagnosticCode domain type. Add closed RustCommand::Explain(DiagnosticCode), mapped
to /opt/rust/bin/rustc --explain <code> --color never; no free flags or host subprocess.
Execute with an empty owned source bundle under the same approved sandbox profile,
clean environment, network deny, process-tree cleanup and joined worker admission.
No project source/handle or Resources is needed. Calibration remains shared/lazy.

A DiagnosticExplainPort returns bounded compiler explanation facts; application
checks cancellation and adds latest_known provenance/freshness for the captured
compiler output artifact. Include immutable image, actual configured Rust version,
configuration/execution fingerprints and content SHA. A complete successful nonempty
response is passed; a well-formed code absent from this compiler is unavailable.
Timeout/overflow/cancellation/cleanup uncertainty preserve existing operational
semantics. Do not fabricate an explanation on unknown code or partial output.

Limit each compiler stream to64KiB and execution work to30seconds, with120second
joined worker deadline including initial calibration. Bound full MCP envelope to
512KiB, otherwise return output limit. stdout remains protocol-only. SourceKind
Artifact describes captured compiler output, not stored ArtifactStore/Resource URI.
No output file, retention authority or project lease is invented for this tool.

## Alternatives considered
Host rustc bypasses the single gateway and depends on unapproved installation.
Embedding static explanations risks mismatching the installed compiler. A heuristic
borrow helper cannot substitute for rustc evidence. Requiring a project_ref is
unnecessary and conflicts with the spec input example.

## Consequences
Runtime identity stays the explicitly approved1.98.1 image. Adding a closed command
changes gateway implementation/configuration fingerprint, requiring actual calibration
and argument/containment tests. This tool has no filesystem authority from MCP and
executes no project code. Native-platform qualification remains unchanged.

## Sources
Official rustc command-line reference, --explain and --color (checked2026-09-04):
https://doc.rust-lang.org/rustc/command-line-arguments.html#--explain-provide-a-detailed-explanation-of-an-error-message
Installed1.98.1 execution remains the version-specific evidence.

Freshness60/300 describes the captured installed-runtime observation; immutable
compiler text is identified independently by content SHA and approved image. It is
not a claim that the explanatory text changes with age. The unknown-code stderr
vocabulary is version-specific1.98.1 and pinned by an actual E9999 runtime test;
future approved image changes must rerun it. Unexpected stderr remains infrastructure
rather than an invented successful/absent explanation.
