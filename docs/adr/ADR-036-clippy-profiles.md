# ADR-036 — Closed Clippy profiles

## Status
Accepted and implemented M1-05, 2026-09-04; evidence in validation/M1-05.md.

## Context
Spec23.5 requires structured Clippy findings and closed workspace/package/features/
all_targets/lint_profile selections. Clippy executes build scripts and proc macros;
Cargo check containment alone is not evidence of the actual Clippy path.

## Decision
Expose live project_ref plus the five specified selections. Package/feature grammar
and conflicts match check; no arbitrary flags, target, all_features or config in
input. lint_profile enum defaults to default. default and project intentionally
both honor the captured native manifest/source lint policy without extra levels.
strict adds only `-- -D warnings` (including rustc warnings); pedantic adds only
`-- -W clippy::pedantic`, opt-in and warning-level, honoring contextual source
allows. Profiles do not guarantee enforcement against adversarial lint suppression.
The reported pass means the configured Clippy command completed successfully.

Execute fixed `cargo clippy --frozen --message-format=json --jobs=1` plus validated
selections/profile in the approved existing Linux ARM64 gateway. The approved
runtime already includes cargo-clippy/clippy-driver; no provisioning required.
Network deny, read-only captured source, env isolation, execution budgets,
calibration, tree cleanup and quarantine stay identical. Execute the actual
build.rs and proc-macro containment fixtures through Clippy as additional evidence.

Reuse bounded Cargo diagnostic parsing and the narrowly shared capture/publication
lifecycle from M1-03/04. Factor only common Cargo result normalization inside the
execution adapter; ports and typed options remain distinct. Diagnostics preserve
clippy:: names, locations and grouped suggestions, treated as project-writable
untrusted output. Exit0 plus complete successful build-finished is passed even
with warning findings; strict may fail those findings. Compiler/lint failure is
failed/isError=false; incomplete and frozen/timeout semantics follow ADR-034.
No fix flag or write path. Freshness, latest_known, opaque logs and quota fallback
apply without contract changes to existing tools.

## Alternatives considered
Global pedantic: explicitly forbidden by specification. Arbitrary lint args: deny
policy violation. Default=strict: surprising warning promotion. Removing project
profile: loses an explicit user choice suggested by spec despite equivalent
native semantics. Separate execution gateway or host Clippy: unnecessary and unsafe.

## Consequences
New public tool with snapshot across five MCP versions, real default/strict/
pedantic/project cases and hostile compile-time fixtures required. Shared Cargo
normalization must preserve check regressions. Snapshot provenance authenticates
captured source, never the origin or completeness of project-controlled messages.
No registry/git dependency downloads, external source cache or platform expansion.

## Sources
- https://doc.rust-lang.org/clippy/usage.html
- https://doc.rust-lang.org/clippy/configuration.html
Pinned approved Clippy0.1.98/Rust1.98.1 real executions determine compatibility.

Diagnostic family normalization uses clippy:: on the root and its descendants;
other roots, including code-less compiler messages, retain the historical rustc
label. This convention is not producer authentication and does not discard
code-less diagnostics or change their severity/completeness.
