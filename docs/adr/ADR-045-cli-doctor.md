# ADR-045 — Explicit local doctor and stable CLI reporting

## Status

Accepted for implementation after M1-13,2026-09-04. M1-14 only; no new MCP tools,
remote server, updater, config-file format, catalog export or M2 capability.

## Context

Specification110/111 requires serve, doctor, version, capabilities and catalog
commands, useful human/JSON diagnostics and explicit host configuration. Existing
serve flags configure trusted roots, signed catalog/model/index, RustSec and the
approved Docker runtime. Catalog commands and active sandbox capabilities already
exist. Generic host PATH tool discovery would escape the execution policy. The
approved runtime intentionally uses an owned RustSec library instead of cargo-audit.

## Decision

Add doctor [same host flags as serve] [--active] [--json]. Reuse a single closed
host-configuration parser for serve and doctor; no inference from project files or
host PATH. Duplicates/incomplete groups/invalid fixed image remain rejected. Root,
TTL, catalog/trust/model/index and RustSec flags retain their semantics. CLI syntax
failures exit2; diagnostic failures exit1; an adequate report exits0. Nothing is
installed, synchronized, repaired or downloaded automatically.

Default doctor observes local configured files through the existing CatalogProvider
and safe filesystem adapters. It may load verified local E5/Lance if configured;
passive means no subprocess probes, not zero I/O or zero native computation. It
reports component identities/freshness and actual access outcomes, never claims an
ACL audit or native Windows/Linux containment. A read-only doctor must not acquire
CatalogStore administration lease or clean staging. Unconfigured optional services
are not failures. A configured required component that fails validation degrades
health and returns exit1. Stale/unknown freshness is reported with an action; it
must not invent a current snapshot or silently refresh.

Runtime inventory and sandbox enforcement remain not_checked in passive mode even
when flags are present. --active explicitly authorizes existing Rust gateway
calibration fixtures and fixed rustc/cargo/installed-component observations inside
the exact approved Linux ARM64 image. This runs no user project. A fixed in-memory
minimal source snapshot feeds the existing ToolchainInspectionPort; no new process
runner or command enum is necessary. It uses existing gateway limits/cancellation,
containment and cleanup. The report binds observed toolchain to runtime image and
execution/configuration fingerprints. Host tools stay not_checked. cargo-audit is
not_used, with the RustSec library engine identified separately, never fabricated
as an observed installed binary. Optional external tools are explicitly unknown.

Typed JSON doctor format_version1 has mode, overall status, finite diagnostic check
IDs/scopes/status/reason codes, bounded action strings and relevant catalog/runtime
evidence. Human output derives from the same report. Reports have bounded encoded
output; deadlines are cooperative and gateway cleanup is joined on normal completion, deadline and handled interruption.
No arbitrary paths/secrets or raw parser/process errors are echoed in diagnostics.
Doctor registers interruption handling before work, runs the synchronous observation
on a joined worker and sets cancellation on SIGINT/SIGTERM/SIGHUP or deadline, then waits
for gateway cleanup. Enable only the existing pinned Tokio signal feature; its
signal-hook-registry dependency is already in Cargo.lock. No new library/version.
Passive deadline120s, active900s; cleanup can extend elapsed time, never falsely
report hard native preemption. Repeated handled signals do not bypass normal cleanup. SIGKILL/crashes remain outside
this cooperative guarantee. Signal observation remains live through report delivery.
Only after all gateway/observation work has joined, a bounded response writer waits
up to5s or a handled signal. A stalled response writer may end with process exit;
it owns no execution/catalog resources. This is distinct from detaching gateway
work, which is forbidden. Delivery failure exits1 and may leave an incomplete
response, rather than an invented success report.
Overall status passed/warning/failed describes this diagnostic run, not universal
MCP readiness. Unconfigured/unchecked optional facilities or stale/unknown freshness
produce warning/exit0; a configured failed component or interruption fails/exit1.
An embedded index whose optional model is not configured or whose local feature
is absent also warns; explicitly configuring an unavailable index still fails.
Corrupt embedded data remains a failure. Deadline-driven runtime cancellation is
labeled deadline, while signals are labeled interrupted.

version retains its existing human line and adds version --json with format_version1,
package version, compiled local feature and target OS/architecture. These are build
facts, not tested platform capability claims. capabilities retains its existing
active default JSON contract; --json is an explicit alias, --human renders the same
report. Existing fully configured invocation remains byte/field compatible. Its
trusted probe image is still separate from the Rust runtime; doctor active is the
Rust calibration path. No misleading capability inventory is inferred at startup.

## Alternatives considered

- Run host cargo/rustup found on PATH: violates deny-by-default execution.
- Catalog admin status as passive doctor: takes lease and may clean staging.
- Silently calibrate while reporting passive status: hides active trusted code.
- Mark missing cargo-audit as audit failure: contradicts the owned RustSec engine.
- Implement HTTP/update/config/export now: explicitly outside immediate M1 CLI.

## Consequences

Host setup can be diagnosed reproducibly without granting a project or model extra
authority. Active verification costs real calibration time and is clearly requested.
The report cannot turn absent native runners, client qualification, licenses,
publisher approvals or utility results into M1 closure. Those remain separate gates.

A worker panic during active observation reports cleanup_uncertain: joining a
panicked worker alone cannot attest explicit gateway teardown. Runtime/signal
initialization failures can exit1 before a report exists; they are infrastructure
startup failures, not completed diagnostics. Repeated queued signals can abort
report delivery after cleanup. Native uncooperative observation can still require
SIGKILL; installing signal handlers deliberately replaces default signal exits.
