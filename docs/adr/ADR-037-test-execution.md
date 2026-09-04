# ADR-037 — Bounded Cargo test execution

## Status
Accepted and implemented, 2026-09-04; current evidence in validation/M1-06.md.

## Context
Spec23.7 requires Cargo test with package/test_filter/features/all_features/target/
timeout, never arbitrary harness commands. This R2 operation executes test binaries
and doctests as well as build scripts/proc macros. Existing check evidence alone
is not an actual test-runtime cleanup oracle. Cargo JSON covers compilation, not
the entire stable libtest/custom harness stream.

## Decision
Expose only live project_ref and the specified options. Reuse closed package,
feature and installed-target grammar from check. test_filter is an optional ASCII
alphanumeric/underscore/colon substring,1..128 bytes, starting alnum/underscore;
no whitespace, leading dash or arbitrary trailing args. timeout is an integer in
seconds1..60, default30, for the gateway job including preflight, source transfer
and compile/test. It excludes application capture, initial calibration and final
cleanup, which have independent joined control budgets. Worker deadline120s still joins actual cleanup.

Fixed command: `cargo test --frozen --message-format=json --jobs=1 --color=never`
plus validated selections/filter and fixed `-- --test-threads=1 --color=never`.
Use native Cargo default target/member selection, including enabled doctests and
custom harnesses. Do not force workspace/all-targets or no-fail-fast. Test-level
ignores, filters, manifest exclusions and custom harnesses affect coverage;
passed means the selected Cargo command passed, never proof that tests exist or
all project tests were exercised. No fabricated test counts/names from arbitrary
human output; retained Resources carry harness details.

Reuse bounded Cargo diagnostic parsing only through the first complete
build-finished record. A successfully compiled build may then produce arbitrary
bounded harness output; retain that tail unchanged in logs. Scan the bounded tail
for additional Cargo events or quoted reason markers (including malformed/interleaved lines): an early forged build-finished or Cargo-looking
harness output makes the phase ambiguous. Mark it incomplete and clear
build_succeeded; never accept it as complete compilation evidence. Failed builds must not have an unexplained tail.
Expose build_succeeded nullable as reported compilation evidence, alongside
validation_complete and execution outcome. Exit0 plus complete successful build
is passed; nonzero exit with a complete build phase is failed/isError=false.
Incomplete parsing/truncation never passes; timeout is blocked with safe partial
evidence. Code can write these streams: no producer authentication. No unstable
libtest JSON flags, and no interpretation of harness text as executable edits.

Use the same approved calibrated sourceRO/network-denied Docker profile and
resource caps. Dedicated actual R2 fixtures verify network/env/fs/cgroups, then
observe detached test descendants before timeout/cancel/overflow and verify full
container/volume cleanup. MCP tests must observe an active test binary before
cancellation/EOF and show responsive discovery, backpressure, worker reuse and
clean shutdown. Never execute hostile source on host. Application uses a distinct
ProjectTestPort with shared capture/publication/Resources authorization.

## Alternatives considered
Treating entire stdout as Cargo JSON rejects valid test output. Unstable libtest
JSON is unavailable on pinned stable. Executing discovered test binaries directly
adds arbitrary program/argv and build artifact trust boundaries unnecessarily.
Parsing human summaries as authenticated coverage is unsound with custom harnesses.
Unbounded user timeout/args or host tests violate containment. Disabling doctests
would silently narrow normal cargo test behavior.

## Consequences
No new image, dependency or downloads. Capability remains the existing approved
Linux ARM64 profile; no native-platform claim.30s default/60s maximum and disk/RAM
limits can reject large suites; that is explicit bounded availability. SourceRO
can make tests that write their package directory fail. Logs retain those valid
failures. Final full semantic gate and later M1 audit/import work remain pending.

## Sources
- https://doc.rust-lang.org/cargo/commands/cargo-test.html
- https://doc.rust-lang.org/rustc/tests/index.html
Pinned Rust/Cargo1.98.1 runtime tests determine actual behavior.

The explicit host Rust runtime tuple remains the opt-in for closed project-code
operations; omitting it denies them. R1 and R2 both require the same demonstrated
strict effects/containment. The ordinal class is not a separate security boundary.
Cargo build-finished precedes doctest compilation: build_succeeded describes that
reported phase only. Later doctest compiler errors remain in the harness log.

Review refinements: fixed harness arguments can make a custom harness that rejects
them fail; this is a valid command failure, not inferred assertion failure.
readOnlyHint describes the enforced host/source effects (read-only source, isolated
ephemeral writes, denied network), not absence of code execution or R2 risk. The
log section headings are human framing, unescaped and forgeable by either stream;
never use them to authenticate stream origin. Structured stream truncation flags
remain authoritative about capture, not about project-generated content.

Completeness means bounded capture/parsing coverage, not producer authentication.
Partial capture/timeout may retain a reported build phase with validation_complete
false. Ambiguous phase markers clear it. No guarantee is made to recognize every
possible byte interleaving by arbitrary project code.
