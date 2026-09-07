# ADR-062 — LLVM coverage accounting and authorized SemVer baselines

## Status

Accepted 2026-09-06 by the M3 orchestrator after independent reviews V06/V17/V18
(ADR-063: owner provisioning authorization 2026-09-05). M3-04 was calibrated and
qualified 18/18 in Q01. ADR-065 authorizes M3-03's dedicated executable target;
its W2 amendment and real Docker qualification passed coverage 8/8 within the
historical M3 runtime gate 55/55 — see docs/validation/M3-03.md. That count is
superseded by the later W6 receipt, whose live M3 runtime count is 62/62.

## Context

D18 (`docs/roadmap/adr-backlog-m2-m8.md`) is a prerequisite of M3-03 (`rust.coverage`)
and M3-04 (`rust.semver.check`) per `docs/roadmap/m3-quality.md`: "Coverage y pares
SemVer permiten trabajo de fixtures independiente tras D18." Both tools are proposed
in spec §26.2/26.4 (`docs/spec/rust-engineering-mcp-propuesta-v0.3.md`) with minimal
shape (`line_percent`/`region_percent`/`functions_percent`/`uncovered`, HTML as
artifacts not response body) and are unimplemented in this checkout: `RustCommand`
(`crates/domain/src/rust_execution.rs`) is closed to
`Metadata|FormatCheck|TestProject|ClippyProject|Check|CheckProject|CompilerVersion|
Explain|CargoVersion|InstalledComponents`; no coverage or semver variant exists.
Reproducibility is required by spec §104 (rustc/cargo/target/features/profile in
every result).

The existing M1 precedent this decision must extend rather than duplicate:

- `RustGateway` (`crates/execution-adapter/src/rust_gateway.rs`) runs one pinned,
  calibrated, network-denied (`--network=none`, `CARGO_NET_OFFLINE=true`), read-only
  source, non-root Docker profile (`APPROVED_RUST_IMAGE`, pinned `rustc`/`cargo`
  1.98.1, no rustup). Every `Phase::arguments()` is a closed argv; adding a
  `RustCommand` variant changes `configuration_fingerprint()` and "requires real
  recalibration" (ADR-033). This is the seam a coverage/semver command must extend
  through — never a new gateway, shell, or free flag.
- `rust_calibration.rs` calibrates the sandbox against real fixture sources executed
  through the same gateway (`execute_calibration` under `Admission::Calibration`),
  never against documentation alone — the pattern D18's SemVer exit-code
  calibration must reuse.
- ADR-037/`rust.test` and ADR-040/`rust.quality.gate` establish: closed selection
  grammar reused from `CheckSelection` (package/features/all_features/target, no
  free flags, no workspace/all-targets); `RuntimeIdentity`
  (`crates/domain/src/inspection.rs`) plus `SourceFingerprint`/`ExecutionFingerprint`
  (`crates/domain/src/value.rs`) as the identity vocabulary; `ToolStatus`
  (`Passed|Failed|Blocked|Unavailable|Cancelled`) and `OperationalErrorCode`
  (`crates/domain/src/result.rs`) as the closed outcome taxonomy; "unknown/partial/
  unavailable/skip never equal pass" applied per-stage (`QualityStageReport::classify`,
  `AuditState::{Unavailable,Incomplete}` never map to `Passed`); one joined worker,
  single source capture, per-stage revalidation without renewing the lease.
- ADR-028 bounds `ArtifactMetadata` (`crates/domain/src/artifact.rs`) to
  `owner/id/sha256/size_bytes/truncated/created_seconds/expires_seconds` today —
  no `kind`/MIME/sensitivity fields yet; those are D17 scope (`m3-quality.md`
  "Artifacts ricos D17"), not decided here.
- ADR-031 fixes the source-capture/ingest model (`SourceBundle`, USTAR-encoded,
  read-only volume mount) that both a coverage run and each of the two SemVer
  captures must reuse unchanged.

Official upstream documentation, fetched read-only by the orchestrator on
2026-09-05 UTC (`sources/index.txt`, all relevant fetches HTTP 200):

- `sources/llvmcov-readme.txt` — cargo-llvm-cov README (raw, main branch).
- `sources/semver-readme.txt` — cargo-semver-checks README (raw, main branch).
- `sources/releases-summary.json` — GitHub Releases API: cargo-llvm-cov v0.9.0
  (`cargo-llvm-cov-aarch64-unknown-linux-gnu.tar.gz`,
  `sha256:9af53b273e50d01d8bde8785de8541f6738cc4375248cd7683aec8b5768b9d21`,
  1,613,151 bytes); cargo-semver-checks v0.50.0
  (`cargo-semver-checks-aarch64-unknown-linux-gnu.tar.gz`,
  `sha256:e35f435ea322659381f52e7034bb4f0470108f5b267d29f13cf08152fa4af29b`,
  7,866,573 bytes); cargo-nextest 0.9.143; cargo-mutants v27.1.0 (x86_64 assets
  only — no aarch64 asset, out of scope for D18 but relevant to the D19-adjacent
  nextest/mutants provisioning work).
- `sources/rust-llvm-tools-sha.txt` — official
  `llvm-tools-1.98.1-aarch64-unknown-linux-gnu.tar.xz`
  sha256 `caaf950c65f3e428247dbe9c173d142b7072b2134962a61924c01e39f6b6dc1e`,
  matching the pinned `rustc`/`cargo` 1.98.1 already approved by ADR-031/033.

The plugins were subsequently provisioned and verified 47/47 by ADR-063 in
approved image
`sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`.
Q01 reused those pinned versions and digests unchanged. This ADR did not itself
authorize installation; ADR-063 remains the provisioning decision and receipt.

## Decision

### 1. Shared plugin/runtime identity

Extend the existing calibrated-identity pattern (ADR-033's `CompilerVersion`/
`CargoVersion`/`InstalledComponents`) with two new closed, argument-free
`RustCommand` variants whose sole purpose is version observation:
`LlvmCovVersion` (`cargo llvm-cov --version`) and `SemverChecksVersion`
(`cargo semver-checks --version`). Each adds a new entry to
`configuration_fingerprint()`'s command list (ADR-033: "Adding a command changes
the gateway configuration fingerprint and requires real recalibration") and is
parsed and compared byte-exact against a new pinned constant analogous to
`APPROVED_RUST_VERSION`/`APPROVED_CARGO_VERSION`, verified only once during
explicit provisioning, never inferred from an installed-file heuristic. A mismatch
(wrong version, missing binary, unexpected output shape) is `Unavailable`
(`OperationalErrorCode::ToolNotInstalled`-class), evaluated *before* attempting a
coverage or semver command, exactly as `rust.dependencies.audit` refuses to
fabricate a corrected version from partial data. `llvm-tools` version/digest is
recorded the same way `InstalledComponents` already records `rust-std-<triple>`
entries: extend that parser to also assert the presence and version of the
`llvm-tools-preview` component, cross-checked against the pinned
`llvm-tools-1.98.1-aarch64-unknown-linux-gnu` sha256 above (`llvm-cov`/
`llvm-profdata` are shipped by that component, per the README's `LLVM_COV`/
`LLVM_PROFDATA` environment variables). Recorded identity tuple for both tools:
`{plugin_version, plugin_binary_sha256, llvm_tools_version, llvm_tools_sha256,
rustc_version, cargo_version}` — this is what a coverage/semver baseline's
"plugin identity" component (§4, §7) refers to throughout this ADR.

### 2. Coverage: two-phase capture is mandatory, JSON is authoritative

The README's "Merge coverages generated under different test conditions" section
and the `--no-report`/`report` subcommand descriptions confirm the required shape:
one `cargo llvm-cov --no-report [selection]` phase builds and runs tests, writing
`.profraw`/merged `.profdata` under `CARGO_LLVM_COV_TARGET_DIR`; the report is then
produced any number of times from that same profdata via `cargo llvm-cov report
--json --output-path ...`, `report --lcov --output-path ...`, `report --html
--output-dir ...`. This two-phase flow — never three independent top-level
`cargo llvm-cov` invocations — is the only way multiple formats are guaranteed to
describe the *same* test execution instance, matching the roadmap oracle's "mismo
run/config" requirement; three independent runs could each re-execute tests and
observe different flaky results, silently breaking that invariant. `report
--json`, `report --lcov`, and `report --html` are three separate, closed `report`
invocations against that one shared profdata, each its own fixed argv (no
combined multi-format single invocation) — the README's own worked example
("`cargo llvm-cov --no-report --features a`... `cargo llvm-cov --no-report
--features b`... `cargo llvm-cov report --lcov  # generate report without
tests`") confirms `report` reads existing profdata rather than re-running
anything, which is the property this two-phase design depends on.

Full JSON (never `--summary-only`) is the authoritative record. `--summary-only`
"can only be used together with --json, --lcov, or --cobertura" and "Export[s]
only summary information for each file" — it drops the per-region/per-function
detail needed for the zero-denominator check (§3) and for merge dedupe (§4).
Domain metrics extracted per file, per package, and for the requested aggregate
scope: `lines{count, covered, percent}`, `regions{count, covered, percent}`,
`functions{count, covered, percent}`. Branch and MC/DC coverage (`--branch`,
`--mcdc`) are excluded: the README states both are unstable/nightly-only, and the
pinned toolchain is stable 1.98.1 with no nightly capability approved by ADR-031.
Doctests (`--doctests`) are excluded by default for the same reason `rust.test`
already excludes unstable libtest features (ADR-037) — proposed default scope for
M3-03 is "no doctest coverage," listed as an Open issue for owner confirmation
rather than decided unilaterally here.

Every coverage result records, explicitly rather than by convention:

- `cargo-llvm-cov` self-reported version and `manifest_path`, read from the JSON's
  own embedded `cargo_llvm_cov: {version, manifest_path}` object (README,
  "Additional JSON information") and cross-checked against the pinned identity
  from §1 — a mismatch is `Blocked`/internal, never silently trusted.
- The effective `--ignore-filename-regex` set: the README's documented default
  excludes (`rustc toolchain paths`, `{tests,examples,benches}` directories,
  `tests.rs`/`*_tests.rs`/`*-tests.rs`, the target directory, `$CARGO_HOME/
  {registry,git}`, `$RUSTUP_HOME/toolchains`) are reproduced verbatim in the
  recorded metadata. No custom `--ignore-filename-regex` is accepted from a
  caller — that would be an unbounded free flag, forbidden by the invariants.
- `cfg(coverage)`/`cfg(coverage_nightly)` state: left at the tool's default (both
  flags un-set, i.e. `--no-cfg-coverage`/`--no-cfg-coverage-nightly` are never
  passed), so project code relying on `#[cfg_attr(coverage_nightly,
  coverage(off))]` behaves as documented.
- rustc/cargo/llvm-cov/llvm-tools versions (§1), target triple (fixed
  `aarch64-unknown-linux-gnu`, matching `APPROVED_RUST_IMAGE`'s platform — no
  `--target` override, no `--coverage-target-only`/`--coverage-host-only`), and
  the package/feature selection actually used, reusing the closed
  `CheckSelection`-shaped grammar (`package`/`workspace`/`features`/
  `all_features`/`no_default_features`/`target`) already validated by
  `CheckOptions::try_from` — no `--exclude`/`--exclude-from-test`/
  `--exclude-from-report`/`--dep-coverage`/`--include-ffi`/`--include-build-script`
  free flags.

### 3. "Zero data is not 100%"

A coverage percent is only ever reported for a scope (file, package, or requested
aggregate) that has at least one executable line/region/function, i.e.
`lines.count > 0` (and independently for `regions.count`/`functions.count`). If a
scope's denominator is `0` — an empty file, a file entirely excluded by
`cfg(coverage)`, or a package with no instrumented code reached by the executed
tests — the domain reports that scope as **absent from the metrics set**, not as
`0.0%` or `100.0%`. This is enforced at the domain contract level the same way
`Provenance`/`FreshnessPolicy` reject inconsistent construction
(`crates/domain/src/evidence.rs`): a `CoverageMetric` type constructed with
`count == 0` and any nonzero `covered`, or with a `percent` present but no matching
nonzero denominator, is a contract violation, not a representable value. This
generalizes ADR-037/040's "unknown/partial/unavailable/skip never equal pass" from
a status field to a metric field.

### 4. Derived LCOV/HTML and retention; JSON is split into artifact and bounded summary (F4, F3)

The full raw `cargo llvm-cov --json` export (never `--summary-only`, per §2) is
**artifact only**: it is retained under the existing Resource/artifact mechanism
and retrieved through a Resource URI, never inlined into MCP structured content,
regardless of size. A real workspace's full per-region/per-function JSON can
trivially exceed the roadmap's 512 KiB complete-MCP-response budget (reused
verbatim by ADR-040/060/061), so treating it as eligible for structured content —
as an earlier draft of this ADR implied — is a contract-level defect, not a
convenience; it is corrected here.

MCP structured content instead carries a **bounded domain summary** derived from
that JSON, never the raw export: the requested aggregate scope's
`lines`/`regions`/`functions` metrics (§2) and each package's own metrics are
always inline. Per-file metrics are a separate, paginated list retrieved through
an artifact/Resource, capped at an explicit maximum row count per page; a
response that cannot include every file's metrics inline or across the returned
page sets an explicit omission flag rather than silently truncating. This mirrors
ADR-061's job-index Resource, which already paginates a similarly unbounded list
under the same 512 KiB budget. The 512 KiB oracle for this contract is a fixture
workspace with a large-enough file count that the naive (unpaginated) per-file
list alone would exceed 512 KiB once serialized with metadata/envelope overhead;
the assertion is that the actual response, paginated, stays within budget and
sets the omission flag rather than being truncated as a false pass (§13 adds this
fixture to the M3-03 oracle list).

LCOV and HTML are artifacts retrieved through the existing Resource/artifact
mechanism, never inlined into MCP structured content — spec §26.2 states this
explicitly ("Los HTML generados deben exponerse como artifacts/resources, no
dentro de la respuesta MCP"), and HTML is hostile content per the invariants (no
script execution or remote resource fetch is asserted by the *server*; a client
that renders the HTML bytes is outside this ADR's control and must be documented
as a caveat, not neutralized). The bounded JSON summary above is the only format
eligible for MCP structured content; the full JSON artifact is retained whenever
the coverage stage reports `Passed` or `Failed`. LCOV/HTML are optional derived
artifacts and may be omitted under quota exactly as ADR-040 already permits
("Quota may omit optional logs explicitly, not authorization") — an omission
never demotes a JSON-backed status, but must set an explicit omission flag
mirroring `QualityStageReport`'s existing `retention_remaining_seconds`/
omission-counter pattern. Persisting the full JSON artifact and LCOV/HTML as
`ArtifactMetadata` requires the `kind`/MIME fields D17 has not yet added
(`crates/domain/src/artifact.rs` today has no such fields) — recording that
dependency is this ADR's job; adding the fields is D17's.

Size bounds (**W2 path-calibrated, not representative-workspace sized**): the full
JSON artifact reuses ADR-028's existing per-artifact byte class. HTML is
inherently multi-file; the packaging mechanism and its ADR-061 `kind` are fixed
jointly below. W2 observed 1,949-byte JSON, 365-byte LCOV and 20,480-byte HTML
USTAR for `known-counts`, with 30,720 bytes the largest HTML fixture. The 8 MiB
ceiling is therefore exercised as a security bound but is not claimed as a
capacity result for a large workspace; see Open issues.

**Multi-file HTML report packaging is one bounded archive blob (joint decision
with ADR-061).** `cargo llvm-cov --html`/`report --html` produces a directory
tree (an `index.html` plus per-file pages and static assets under
`--output-dir`), not one file. Cross
that multi-file report to the host as **one bounded archive blob**, produced and
revalidated the same way M2's mutation candidate export already is, not as an
ad hoc "tar it up" step:

- **Guest side**: a fixed guest program invocation — `tar` with a closed,
  non-configurable argv over the fixed guest report directory — mirrors the
  `MutationPhase::Export` pattern in `mutation_gateway.rs` (`--create --file=-
  --format=ustar --sort=name --one-file-system --directory=<fixed report root>
  .`), run as its own phase after the report phase, over the fixed guest path the
  approved plugin/runtime profile already declares for coverage HTML (ADR-061's
  "closed table of fixed guest paths"). No guest-selected path, glob, or
  compression flag is accepted.
- **Host side**: the resulting stream is decoded and revalidated with the exact
  closed USTAR profile `mutation_archive.rs` already implements (fixed header
  fields, `ustar\0` magic, owner/mode allowlist, strict path validation rejecting
  traversal/absolute/symlink entries, bounded entry count and total bytes) rather
  than a generic/permissive tar reader. A stream that fails any check is denied
  before any bytes are persisted — the same fail-closed posture
  `mutation_archive::decode` already enforces for a hostile candidate export.
- **Re-encoding and storage**: the revalidated set of files is re-encoded
  canonically (fixed header field order/values, sorted member order) using the
  same encoder shape as `mutation_archive::encode`, and the result is stored as a
  **single** artifact/quality-artifact member of kind `ArchiveBundle` (MIME
  `application/x-tar`). It is never previewed, tokenized, or extracted to a host
  path; it is retrieved only as an opaque blob through the existing bounded
  egress/Resource chunking, exactly like any other blob member.

This directly resolves the contradiction with ADR-061's HTML content policy: that
policy's tokenizing sanitizer/preview pipeline (`<script>`/external-URI
rejection) is defined for a `kind: Html` member's bytes being HTML markup, which
a tar archive is not. `ArchiveBundle` is a distinct kind from `Html` precisely so
ADR-061's HTML tokenizer is never asked to run on archive bytes: `ArchiveBundle`
members are always `application/x-tar`, never previewed, and their sensitivity
classification follows ADR-061's ordinary `Operational`/`SourceDerived` rules
applied to the archive as a whole. ADR-061 must add this `kind` variant (and its
"never previewed" content-policy row) before either ADR is accepted; this ADR
supplies the packaging mechanism ADR-061's descriptor schema then references.
Mutation HTML/diff bundles that are already multi-file (if any) reuse the same
`ArchiveBundle` kind rather than inventing a second one.

The 8 MiB ceiling from §4 applies to the final validated `ArchiveBundle` blob,
not the guest's raw tar stream. W2 measured the fixture sizes above; a large
representative workspace remains an operational sizing open issue.

### 5. Merge rules for multi-package coverage

Two or more packages' coverage may only be merged into one aggregate if they come
from the *same* `--no-report` capture: identical `source_fingerprint` of the
captured `SourceBundle`, identical target triple, identical per-package feature
selection, and identical instrumentation/cfg state (§2). This mirrors ADR-040's
"compare all returned source fingerprints... inconsistency is infrastructure,
never a combined snapshot" — two independently captured generations are never
merged even if their reported percentages happen to coincide. A source file
physically shared between two workspace packages (e.g. via a `path = "../common"`
dependency compiled into both) is counted **exactly once** in any
workspace/aggregate-scope rollup: dedupe by canonical relative file path before
summing `lines`/`regions`/`functions`. Per-package views may still show the shared
file (expected duplication at that finer granularity); only the aggregate must not
double-count it. A merge is refused (reported `Blocked`/`Incomplete`, never a
silently wrong aggregate) if the two captures used different effective
`--ignore-filename-regex` sets, different `--doctests` state, or different
`cfg(coverage)` state.

### 6. Coverage baseline identity

A coverage "baseline" is identified by the tuple `(source_fingerprint,
package/feature/target selection, plugin+llvm identity per §1)`, never by a branch
name or other human label — the roadmap states this explicitly ("Baseline de
cobertura identifica source/config, no solo branch name"). Two captures of the
same `SourceBundle` under different feature flags are different baselines by
definition; comparing coverage across them is not decided by D18 and is out of
scope for M3-03 (no coverage-diff tool is proposed here).

### 7. Coverage outcome taxonomy

Reuses `ToolStatus` unchanged: `Unavailable` when the pinned plugin/llvm-tools
identity (§1) is absent or mismatched; `Blocked` for timeout, output-limit,
incomplete JSON parse, or a scope that would require reporting a percent over a
zero denominator (§3) — mirroring `QualityIssue::Incomplete`; `Failed` is reserved
for a future, explicitly configured threshold breach (`--fail-under-lines` et al.)
— D18 does **not** decide whether `rust.coverage` enforces a minimum percent; that
is an M3-03 scope decision (Open issues); `Passed` only when the JSON parses
completely, every requested scope with a nonzero denominator is present, and the
plugin/llvm-tools self-reported identity matches the pinned constants.

### 8. SemVer input contract

Two `ProjectRef`s (baseline, candidate), each independently authorized and live,
captured in a **stable lock order** — always baseline before candidate — through
the existing `source_inner`/`resolve_inner` flow, with a `resolve_inner`
revalidation immediately after each capture, mirroring ADR-040's per-stage
revalidation-without-lease-renewal pattern. The fixed order prevents a TOCTOU class
where two concurrent captures interleave and one `ProjectRef`'s identity changes
between the two reads. Each capture produces its own `SnapshotEvidence` (own
`created_at`/`observed_at`); unlike quality-gate's single shared outer evidence,
baseline and candidate are legitimately two different sources and are **not**
required to share one fingerprint — the roadmap calls this out directly ("snapshot
identity distinta por root; no afirmar atomicidad entre roots externas"): no
combined atomic instant across the two captures is ever claimed.

No URL, registry version, or Git ref is accepted. The README documents four ways
to supply a baseline — `--baseline-version` (crates.io lookup, requires network),
`--baseline-rev` (git revision, requires a `.git` history and walks up the
filesystem via `GIT_DIR`/`GIT_CEILING_DIRECTORIES`/
`GIT_DISCOVERY_ACROSS_FILESYSTEM` detection), `--baseline-root` (a local directory
containing baseline crate source), and `--baseline-rustdoc` (a pre-built rustdoc
JSON file). Only `--baseline-root` is used, pointed at the baseline `SourceBundle`
materialized inside the guest at a second fixed path, mounted read-only exactly
like the candidate.

**This requires a real, explicit extension to `RustGateway`, not a reuse of the
existing seam unchanged (F2).** Verified against `crates/execution-adapter/src/
rust_gateway.rs`: `RustGateway::execute`/`execute_observed` (`execute_observed`,
~lines 746-904) accept exactly one `source: &SourceBundle`, create exactly one
Docker volume, run exactly one `Phase::Ingest` extracting to the hardcoded
`--directory=/source`, and `arguments()` (~lines 473-533) emits exactly one
`--mount=type=volume,...,target=/source...` per container — the same
one-volume/one-mount shape `mutation_gateway.rs`'s parallel `MutationPhase`/
`create_arguments` uses for its own single `/source` mount. Running `cargo
semver-checks --baseline-root /baseline` against a candidate at `/source`
requires, in the same container, **two** simultaneously mounted read-only
volumes plus a second ingest phase populating `/baseline`. Concretely, the
gateway needs:

- a second `Volume` value (baseline volume) created and inspected alongside the
  existing candidate volume, both read-only for the `Run` phase;
- one or two new closed `Phase` variants — e.g. `Phase::IngestBaseline` (or a
  generalized `Phase::Ingest(Target)` distinguishing `/source` vs `/baseline`) —
  reusing the exact same `tar --extract ... --no-same-owner --no-same-permissions
  --keep-old-files` argv already used for `/source`, pointed at `/baseline`;
- `arguments()` gaining multi-mount support: today it emits one `--mount=...`
  literal per container; a `RustCommand::SemverCheck` invocation must emit two
  (`/source` candidate, read-only for the `Run` phase, plus `/baseline`,
  always read-only), which is a change to that method's shape, not only its
  data;
- a `configuration_fingerprint()` update, since it already enumerates every
  `Phase`'s `arguments()` (§1's fingerprinted command list) — adding the new
  phase/mount shape changes the fingerprint and "requires real recalibration"
  (ADR-033), exactly as any other `RustCommand` addition does.

The candidate keeps its existing single mount at `/source`, unchanged; the
writable `/work` tmpfs remains the only writable location in the container —
`/baseline` is a second **read-only** mount, never writable, so this extension
does not add a second writer alongside untrusted code. This is a genuine,
scoped extension of the one closed `RustGateway`/`RustCommand` seam (new `Phase`
variants, a second `Volume`, multi-mount `arguments()`), not a new gateway,
image, or execution model; the Consequences section below is corrected to say so
explicitly rather than claim no execution-model change at all.

`cargo-semver-checks`'s git auto-detection is neutralized by fixing, not
deferring, the mechanism (F7): the semver `RustCommand`'s environment allowlist
(alongside `crate::rust_gateway::environment()`'s existing closed list) adds
`GIT_DIR=/nonexistent` — a fixed guest path guaranteed not to exist in
`APPROVED_RUST_IMAGE` — plus `GIT_CEILING_DIRECTORIES=/` as defense in depth.
Per the README, setting `GIT_DIR` explicitly is authoritative and stops the
upward filesystem walk from `/source`/`/baseline` entirely (`GIT_DISCOVERY_
ACROSS_FILESYSTEM` and `GIT_CEILING_DIRECTORIES` only matter when `GIT_DIR` is
unset, but are added anyway since defense in depth costs nothing here). Since
neither `--baseline-version` nor `--baseline-rev` is ever passed (§8, above),
git detection is not a functioning code path regardless, but a `.git` directory
that happens to exist inside a captured `SourceBundle` (candidate or baseline)
must still never be discovered or used by the tool's own default detection —
§10's calibration protocol adds a fixture asserting exactly that (a `.git`
directory placed inside `/source`'s captured source is never used as a git
baseline source).

Selection must be identical between baseline and candidate: reuse the closed
`CheckSelection`-shaped grammar (package/features/all_features/target) applied
identically to both the rustdoc-JSON-generation step and the `cargo semver-checks`
invocation itself. A request specifying different selections for baseline vs.
candidate is rejected at the domain contract level before any execution —
analogous to `TestOptions::try_from`'s exhaustive validation — never left to
diverge at runtime. Network is denied by the same `--network=none` /
`CARGO_NET_OFFLINE=true` profile as every other `RustCommand`; since
`--baseline-version`/`--baseline-rev` are never passed, this is defense in depth
rather than a functioning code path, but the calibration protocol (§10) must still
exercise a scenario that would otherwise attempt a network fetch, to confirm the
tool reports a distinct, recognizable failure (README's exit 101, "a rustdoc or
build failure, or a connectivity problem") rather than hanging.

### 9. SemVer outcome taxonomy (calibrated in Q01)

The README documents exit codes `0` (no deny-level violation), `100` (one or more
deny-level violations found), `101` (could not complete — rustdoc/build failure or
connectivity problem), and warns that "Command-line parsing errors may use a
different non-zero exit status." Mapping proposed, **pending in-guest
calibration**:

| exit | domain outcome | `ToolStatus` |
| --- | --- | --- |
| 0 | `no_break` (subject to any project-configured `warn`-level findings surfaced as non-blocking, per the tool's own deny/warn split) | `Passed` |
| 100 | `breaking` | `Failed` |
| 101 | `incomplete` (tool/build/rustdoc/connectivity failure) | `Unavailable` |
| anything else | unrecognized outcome — treated as a contract violation requiring investigation | `Blocked` |

Additional causes required by the roadmap oracle text ("herramienta ausente o
parser/version distinto es incomplete/unavailable"), detected *before* invoking
the tool where possible:

- **Tool absent** (pinned binary missing or digest mismatch, §1) → `Unavailable`.
- **Lib target missing**: `cargo-semver-checks` needs a library crate-type to
  generate rustdoc JSON. Detected by inspecting the already-captured
  `ProjectStructure.packages[].targets` (ADR-032) for a `Lib`/`Rlib`/`Dylib` kind
  *before* paying for a doomed rustdoc run → `Unavailable` with a specific reason.
- **rustdoc JSON generation failure** (nightly-format drift; the README notes new
  formats can take "several days to several weeks" to gain support) →
  `Unavailable`, never conflated with a real API break.
- **Version mismatch** between the two captures' recorded rustc/plugin identity
  (should not occur, since both run through the same immutable image, but is
  checked defensively as a combined-identity guard analogous to
  `quality_runtime_matches`) → `Blocked`.
- **Parser failure** (the tool's own internal error interpreting its rustdoc
  input) → `Unavailable`.

Two further defensive rules close a discriminating-oracle gap the review
identified (F6): a `ToolStatus` is never trusted when the parsed finding
evidence contradicts the exit code, in either direction.

- **Exit 0 with warn-level-only findings**: per the README, `warn`-level
  findings never affect exit status, so a run with only warn-level findings
  also exits `0`. This remains `Passed`/`no_break`, but the warn-level findings
  parsed by §11's mechanism must still be surfaced on the report (via the
  finding-count summary and, when available, the best-effort per-finding list) —
  never silently dropped because the exit code alone looked clean.
- **Exit 100 observed with zero parsed findings**: if §11's parser (whichever
  branch calibration selected) finds no findings at all despite exit `100`
  indicating deny-level violations exist, this is `Blocked`, not `Failed` —
  applying the same "unknown/partial/unavailable/skip never equal pass"
  philosophy in the opposite direction: a failure verdict is never reported
  without at least some parsed evidence backing it. Symmetrically, an exit `0`
  observed together with parser-detected deny-level findings is also `Blocked`
  rather than trusted as `Passed`, since the two signals directly contradict
  each other and neither is assumed correct over the other.

### 10. Exit-code calibration protocol and Q01 result

Exit codes were **not** copied from the README into a closed mapping. Q01 executed
this protocol with cargo-semver-checks 0.50.0 in the approved image. Observed
results were: identical/warn-only/compatible cases `0`, deny breakage `100`, and
no-lib/broken-baseline/registry-required `101`; cancellation publishes no exit and
remains typed `Cancelled`. The 18/18 selections and raw shapes are recorded in
`docs/validation/M3-04-semver-calibration.md` and the immutable consolidated
receipt `docs/validation/M3-runtime.json`. The protocol retained below is the
recalibration procedure for a future binary/image change:

1. Provision the pinned `cargo-semver-checks` v0.50.0
   `aarch64-unknown-linux-gnu` binary (sha256
   `e35f435ea322659381f52e7034bb4f0470108f5b267d29f13cf08152fa4af29b`) into the
   approved guest image; this changes `APPROVED_RUST_IMAGE` and requires its own
   provisioning ADR (Open issues) before calibration can run for real.
2. Record the pinned binary's own `cargo semver-checks --help` and
   `cargo semver-checks check-release --help` (or whichever subcommand is
   actually invoked) output verbatim, before running any fixture, per §11 step
   1 — this determines which branch of §11's decision tree applies and must be
   captured as evidence regardless of which branch is taken.
3. Build a fixed, embedded (not host-path) calibration fixture set, mirroring
   `rust_calibration.rs`'s pattern of `include_str!`-embedded fixture sources:
   (a) baseline == candidate (expect exit 0); (b) baseline with a `pub fn` present,
   candidate with it removed (expect exit 100); (c) baseline with no `[lib]`
   target (expect the tool's own "could not complete" path); (d) a baseline root
   pointed at a nonexistent/unparsable manifest (drives the same path via a
   different cause); (e) a scenario that would otherwise require a network fetch
   (e.g. a dependency baseline resolvable only via `--baseline-version`) run
   through the network-denied gateway, to confirm the "connectivity problem" exit
   is reached deterministically rather than hanging; (f) baseline == candidate
   except a lint configured/observed as `warn`-level (e.g. a warn-level-only
   change) to confirm exit `0` while findings are still surfaced (§9); (g) a
   `.git` directory placed inside the captured `/source` `SourceBundle` (and,
   separately, inside `/baseline`) alongside an otherwise ordinary baseline/
   candidate pair, asserting the run behaves identically to the same fixture
   without a `.git` directory present — confirming `GIT_DIR=/nonexistent`/
   `GIT_CEILING_DIRECTORIES=/` (§8) actually prevent the tool's git
   auto-detection from ever using it.
4. Run each fixture through the real pinned binary inside the actual approved
   guest, via the same `execute_calibration`/`Admission::Calibration` path
   `rust_calibration.rs` already uses, and record observed exit code, stdout/
   stderr shape, and any diagnostic JSON for each case.
5. Assert the observed exit codes match §9's hypothesis *before* trusting it in a
   closed `RustCommand` argv/outcome mapping; any divergence is a hard blocker for
   M3-04, to be re-opened as a finding against this ADR rather than silently
   reconciled. Assert fixtures (f) and (g) match the behavior §9/§8 already
   commit to (defensive `Blocked` handling and git-neutralization respectively);
   any divergence is the same kind of hard blocker.
6. Record the calibration run's own output as evidence (a `validation/M3-04-*.md`-
   style artifact), including the pinned binary's sha256, produced by M3-04.

### 11. SemVer report fields — calibrated fallback extraction (F1)

The official `sources/semver-readme.txt` documents no JSON, `--output-format`, or
other machine-readable findings flag anywhere for `cargo-semver-checks` v0.50.0;
the only JSON mentions are rustdoc's own *input* format and `--baseline-rustdoc
<JSON_PATH>` (an input flag). The independently fetched
`obi1kenobi-cargo-semver-checks-Cargo.toml.txt` confirms this absence indirectly:
its dependencies are `trustfall`/`trustfall_rustdoc` (the lint engine),
`handlebars` (human-readable templated terminal reports), `anstream`/`anstyle`
(terminal color), and no machine-readable-output crate. An earlier draft of this
ADR treated per-finding `item`/`lint`/`required_update`/`level`/`span` fields as
if they were straightforwardly extractable; that premise is **unconfirmed** and
is corrected here to a decision tree M3-04 must execute in order, rather than an
assumed capability:

1. At calibration (§10), record the pinned binary's own `cargo semver-checks
   --help` and `cargo semver-checks check-release --help` output (or the
   subcommand actually invoked) verbatim, as evidence — never trust training
   data or the README's absence of documentation as proof a flag doesn't exist;
   `--help` is the authoritative source for this pinned version.
2. If that `--help` output reveals a machine-readable findings flag (JSON or
   otherwise) not mentioned in the README, adopt it and record the exact flag and
   observed schema as evidence before relying on it; the per-finding fields below
   then apply as originally drafted, sourced from that structured output.
3. If no machine-readable findings output exists (the expected outcome given the
   dependency evidence above), M3-04 does **not** attempt full per-finding
   extraction from colored terminal text. Instead:
   - `rust.semver.check` publishes a **coarse authoritative summary** derived
     from data that does not require fragile text scraping: the exit-code-derived
     outcome (§9) plus deny-level and warn-level **finding counts**, obtained from
     a bounded, calibrated text parser over the tool's own non-colored output
     (color forced off via an environment variable and/or CLI flag, whichever the
     pinned binary's `--help` confirms at calibration; `cargo-semver-checks`'s
     `anstream`/`anstyle` dependency auto-detects a non-terminal stream, which
     calibration must also confirm rather than assume). This parser follows the
     same bounded/golden-tested-fixture discipline and fallback rules as spec
     §79: fixed byte/line/field limits, no unbounded buffering, and a fixture
     corpus of real captured output (not hand-written approximations) whose
     expected counts are asserted exactly.
   - Per-finding `item`/`lint`/`required_update`/`level`/`span` fields are then
     **best-effort**: attempted from the same non-colored text via the same
     bounded parser, marked with `completeness: Partial` on the report (never
     `Complete`), and the raw non-colored tool output is always retained as an
     artifact alongside the summary so a caller can inspect what the parser could
     not fully structure.
   - A parser failure (output that does not match any recognized shape within the
     bounded limits) yields `completeness: Incomplete` for the finding list and
     never fabricates a false "no break" — this is the same "unknown/partial/
     unavailable/skip never equal pass" invariant §7/§9 already apply to status,
     now applied to the per-finding field set itself. A parser failure does not
     by itself change the `ToolStatus` derived from the exit code (§9); it only
     bounds how much per-finding detail is trustworthy.

Whichever branch calibration selects, `limitations`: an explicit per-side
feature-set descriptor is retained regardless of the findings-extraction
mechanism, since it is derived from the request's own selection (§8), not from
tool output: the tool's own default heuristic excludes features named
`unstable`, `nightly`, `bench`, `no_std`, and features prefixed `_`,
`unstable-`, or `unstable_` unless the caller opts in via `--features`/
`--all-features`/`--default-features`/`--only-explicit-features` (README, "What
features..." FAQ) — recording the resolved feature set per side lets a caller
distinguish "checked, present on both sides, passed" from "excluded by the default
heuristic, not checked."

Q01 recorded the pinned `--help`: no machine-readable findings output exists, so
branch 3 is selected. The bounded parser is pinned to captured non-colored output
(`--- failure|warning <lint>: <message> ---`, the `checks: ... pass/fail/warn/skip`
summary, and `Summary no semver update required`). Best-effort rows remain
`Partial`, raw output remains authoritative supporting evidence, and any unknown
shape remains `Incomplete`.

### 12. SemVer fixture families (M3-04 requirement, one positive control each)

1. **Removed `pub fn`** → deny/major, exercises `function_missing`.
2. **Trait method added**: two sub-cases, not one — added *with* a default impl
   (non-breaking/minor for implementors) and added *without* a default impl
   (breaking/major) — proving the tool distinguishes them.
3. **Enum variant added, non-exhaustive vs. exhaustive**: a matched pair —
   baseline enum marked `#[non_exhaustive]` (adding a variant is non-breaking) vs.
   a plain baseline enum (adding a variant is breaking) — proving the tool reads
   the attribute rather than always flagging variant additions.
4. **Feature-gated item removed**: an item behind `#[cfg(feature = "x")]` in the
   baseline, removed in the candidate; run once with `--features x` on both sides
   (breakage detected) and once without (the item was excluded by the default
   heuristic and must report no signal for it, not a false "no break").
5. **Incompatible baseline**: a baseline manifest `cargo-semver-checks` cannot
   doc-build at all (missing dependency, syntax error, or no `[lib]` target) →
   must produce `Unavailable`, never a false "no break."

### 13. Coverage/oracle test list for M3-03

- Positive control: a fixture crate with a known, hand-computed line/region/
  function count and a deliberately uncovered branch, asserting the JSON's counts
  match exactly (not just "some number was returned").
- A file physically shared between two workspace packages (§5), asserting the
  workspace-aggregate count is not doubled while per-package views still show it.
- A zero-denominator file (entirely excluded via the default `tests/` exclusion or
  an empty module) asserting it is absent from percent-bearing metrics (§3), not
  `0%`/`100%`.
- A two-phase `--no-report` + `report --json/--lcov/--html` run asserting all
  three formats' totals agree, since they must derive from the same profdata.
- A plugin/llvm-tools identity mismatch (simulated wrong version string) asserting
  `Unavailable`, never a best-effort report.
- Timeout mid-instrumented-build asserting `Blocked`, never a partial `Passed`.
- A large-file-count fixture (§4, F4): a workspace synthesized with enough files
  that the unpaginated per-file metrics list alone would exceed the 512 KiB
  complete-MCP-response budget once serialized with metadata/envelope overhead,
  asserting the actual response stays within budget by paginating the per-file
  list through an artifact/Resource with the explicit omission flag set, never by
  silently truncating the list as a false-complete summary.

## Alternatives considered

- **`--summary-only` as the authoritative JSON.** Rejected: loses per-file/region
  detail required for the zero-denominator rule (§3) and merge dedupe (§5).
- **Three independent `cargo llvm-cov` invocations for JSON/LCOV/HTML.** Rejected:
  each is a separate test execution that can observe different (flaky) results,
  breaking the "same run" invariant the roadmap requires.
- **`--baseline-version`/`--baseline-rev` for SemVer.** Rejected: the former
  requires a live crates.io network dependency incompatible with `--network=none`;
  the latter assumes a `.git` worktree the captured flat `SourceBundle` does not
  provide and risks accidental host/guest git discovery via `GIT_DIR` search.
- **Trusting the README's documented exit codes without in-guest calibration.**
  Rejected: the task and roadmap explicitly forbid copying numbers from
  documentation or another version; the README itself notes rustdoc-JSON/MSRV
  drift risk across versions.
- **Merging coverage across differently configured runs for a "friendlier"
  aggregate.** Rejected: produces a silently wrong percentage with no way for a
  caller to detect the inconsistency.
- **Defaulting SemVer to `--all-features`.** Rejected: overrides the upstream
  tool's own default heuristic (excluding `unstable`/`nightly`-named features)
  silently, changing what "no break" means without operator visibility; the
  closed selection grammar (§8) instead surfaces whatever selection was actually
  used, explicitly, in `limitations` (§11).
- **A single new `RustCommand::Coverage`/`RustCommand::SemverCheck` producing a
  best-effort result on any plugin absence.** Rejected: violates "unknown/partial/
  unavailable/skip never equal pass"; absence must be a distinguishable
  `Unavailable`, checked before execution using already-captured
  `ProjectStructure` data where possible (§9).

## Consequences

No new gateway, image, or execution model is introduced, but SemVer is not a
free-form reuse of the existing seam either: coverage and semver both extend the
existing single closed `RustGateway`/`RustCommand` seam, and SemVer's
baseline/candidate pair requires a real, scoped extension of that seam — a
second read-only `Volume`, one or two new `Phase` ingest variants, and
multi-mount `arguments()` (§8) — not merely new `RustCommand` variants over the
unchanged one-volume shape. Their provisioning is a separate, explicit
image-update ADR (Open issues, now ADR-063) — this ADR does not authorize
installing either plugin. Two new argument-free version-probe commands (§1) and
the dual-mount SemVer commands (§8) join the gateway's fingerprinted command
set, requiring real recalibration once provisioned, exactly as any other
`RustCommand` addition would.

Coverage responses are honest about instrumentation scope: no client can receive a
percent computed from a zero denominator, and multi-package aggregates cannot
silently double-count shared files or blend incompatible runs. This is stricter
than the minimal spec §26.2 JSON shape and requires the domain to model coverage
metrics as a validated type family (§3), not a bag of floats — extra domain/
application work beyond the bare tool wrapper, scoped to M3-03.

SemVer responses never claim atomicity between two independently owned
`ProjectRef`s, and never accept a registry/Git-resolved baseline — this is more
restrictive than `cargo-semver-checks`'s own default UX (which defaults to a
crates.io lookup) and requires every M3-04 caller to have already captured and
authorized both a baseline and a candidate project, which is more setup than
"lint my crate before publish" but matches the invariant that authority always
comes from the host, never from an external registry/version string.

The Q01 calibration closed the SemVer uncertainty: §9's mapping and §11's fallback
parser are pinned to the real guest binary and captured non-colored output. A
future plugin or image change must repeat §10 before either mapping is trusted.

D17's quality-artifact store is now integrated. M3-03 publishes JSON, LCOV and
the validated HTML `ArchiveBundle` through Stage 1 when configured, with the
bounded Stage 0 fallback; W2 qualifies all three formats from one capture.

## Sources

- `cargo-llvm-cov` README (raw, `main`, fetched 2026-09-05T23:47:36Z, HTTP 200):
  `https://raw.githubusercontent.com/taiki-e/cargo-llvm-cov/main/README.md`
  (local copy: `sources/llvmcov-readme.txt`).
- `cargo-llvm-cov` latest release (GitHub Releases API, fetched
  2026-09-05T23:47:37Z, HTTP 200): `v0.9.0`,
  `https://github.com/taiki-e/cargo-llvm-cov/releases/tag/v0.9.0`
  (local copy: `sources/releases-summary.json`).
- `cargo-semver-checks` README (raw, `main`, fetched 2026-09-05T23:47:37Z, HTTP
  200): `https://raw.githubusercontent.com/obi1kenobi/cargo-semver-checks/main/README.md`
  (local copy: `sources/semver-readme.txt`).
- `cargo-semver-checks` latest release (GitHub Releases API, fetched
  2026-09-05T23:47:37Z, HTTP 200): `v0.50.0`,
  `https://github.com/obi1kenobi/cargo-semver-checks/releases/tag/v0.50.0`
  (local copy: `sources/releases-summary.json`).
- Official `llvm-tools` 1.98.1 aarch64 tarball checksum (fetched
  2026-09-05T23:47:40Z, HTTP 200):
  `https://static.rust-lang.org/dist/llvm-tools-1.98.1-aarch64-unknown-linux-gnu.tar.xz.sha256`
  (local copy: `sources/rust-llvm-tools-sha.txt`).
- `cargo-semver-checks` `Cargo.toml` (dependency manifest, confirming the
  absence of a machine-readable-output crate and the presence of
  `handlebars`/`anstream`/`anstyle` for §11's F1 revision; local copy:
  `sources/obi1kenobi-cargo-semver-checks-Cargo.toml.txt`).
- `docs/roadmap/m3-quality.md` (M3-03/M3-04 cuts, coverage/semver oracle
  paragraphs), `docs/roadmap/m2-m8.md` (G1–G9), `docs/roadmap/adr-backlog-m2-m8.md`
  (D16, D17, D18).
- `docs/spec/rust-engineering-mcp-propuesta-v0.3.md` §26.2, §26.4, §79, §104, §105.
- ADR-012, ADR-028, ADR-030, ADR-031, ADR-032, ADR-033, ADR-037, ADR-040,
  ADR-060, ADR-061.
- `crates/domain/src/{rust_execution,check,test_run,inspection,execution,value,
  result,quality,evidence,resolution,artifact}.rs`,
  `crates/application/src/{quality,test_run,source,inspection}.rs`,
  `crates/execution-adapter/src/{rust_gateway,rust_calibration,mutation_gateway,
  mutation_archive}.rs` (the `RustGateway`/`Phase`/`Volume`/`arguments()` shape
  for §8's F2 dual-mount extension, and the `MutationPhase::Export`/USTAR
  encode-decode pattern §4's F3 archive packaging reuses).
- Independent review `docs/validation/m3-delegation/V18-adr062-review/
  last-message.md` (sha256
  4868d2d25bfe57748f31ad49de9084370c5d8929036880a89a21d1578c603ae4), findings
  F1–F7, addressed throughout this revision.

## Open issues

- **Coverage target and accounting are measured.** W2 passed coverage 8/8 inside
  the historical M3 runtime 55/55 attempt. The amended ADR-065 target is
  read-write only in run/report,
  read-only in the keeper and absent from export/non-coverage phases. The real
  known-counts oracle is lines 4/4, regions 8/9 and functions 2/2; shared-file
  aliases normalize to one aggregate entry; no instrumentable code yields
  `no coverage data found` and no fabricated percentage.
- **The 8 MiB HTML/LCOV ceiling is only lightly measured.** Real W2 fixture
  outputs were JSON 1,949 bytes, LCOV 365 bytes and HTML USTAR 20,480 bytes for
  known-counts; the largest observed HTML fixture was 30,720 bytes. This validates
  the packaging path, not capacity for a representative large workspace, so the
  8 MiB bound remains a conservative security ceiling rather than a sizing claim.
- **Fail-under thresholds are undecided.** Whether `rust.coverage` ever produces
  `Failed` from a percent threshold (`--fail-under-lines` et al.) is not decided
  by D18; none of those flags are approved for use. If M3-03 wants this, it needs
  its own explicit decision and closed selection surface.
- **Doctest coverage scope is undecided.** §2 proposes excluding doctests from
  M3-03 entirely (nightly-only per the README's Known limitations, and consistent
  with `rust.test`'s existing exclusion of unstable libtest features); this is a
  proposal pending explicit owner confirmation, not a closed decision.
- **Coverage `Failed`/threshold and cross-baseline coverage comparison are both
  explicitly out of scope for D18** (§6, §7) and would each need their own future
  decision if a caller ever needs them.
