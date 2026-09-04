# ADR-035 — Captured formatting check

## Status
Accepted and implemented M1-04, 2026-09-04; evidence in validation/M1-04.md.

## Context
M1 requires `rust.fmt.check`, affected files and a reasonably small diff without
source modification. The approved immutable Linux ARM64 runtime includes rustfmt
1.9.0 / Rust 1.98.1. Stable cargo-fmt rejects JSON message format with --check;
its human diff requires bounded normalization and real-version verification.

## Decision
Expose only a live `project_ref` input. Check every workspace member with the
closed command `cargo fmt --all --check -- --color never --config disable_all_formatting=false`.
Use the existing calibrated Docker gateway with read-only captured source,
network enforcement, empty fixed environment, fixed RUSTFMT, 30s and 256KiB per
stream. No project code executes for formatting, but the stronger existing
containment remains in force. Do not weaken network or launch a host formatter.

Honor stable project formatting configuration and rustfmt skip attributes;
override disable_all_formatting=false to prevent a whole-project no-op. Report
this as configured workspace formatting, not proof that every source byte was
formatted. Unknown/unstable configuration warnings or parser failures cannot
produce a complete pass. No arbitrary configuration or flags in the tool input.

Normalize pinned human diff headers only to paths of captured regular files.
Handle newline-only differences. Body lines must have the rustfmt context/add/
remove prefix; embedded source text is untrusted. Up to 128 unique affected files,
with explicit omission count, and a whole normalized diff only up to 32KiB;
otherwise omit it explicitly and retain the bounded combined log via existing
live-authorized Resources. Never apply this display diff as an edit. Exit zero
requires empty complete stdout/stderr; exit one is a complete formatting failure
only for a fully recognized nonempty diff and empty stderr. All other output is
incomplete, never passed. Timeout and partial-output semantics follow ADR-034.

Extract the existing application capture/publication lifecycle narrowly so check
and format share final ProjectRef/retention authorization, balanced bounded logs,
quota fallback, cancellation and freshness. Domain/application remain independent
of Cargo/rmcp/filesystem adapters. Worker admission and joined cleanup precede all
work; tool discovery and Resource budgets remain bounded.

## Alternatives considered
JSON or checkstyle/unstable emit modes: not supported by the pinned stable check
contract. Host rustfmt: bypasses the approved gateway. Ignoring all project style:
unnecessary behavior divergence. An unrestricted args/config option: forbidden.

## Consequences
Formatting uses the already approved strict Linux profile (stronger than required
restricted). No new runtime, dependencies or downloads. Formatting errors remain
valid tool results, with validation_complete distinguishing incomplete evidence.
Configuration identity changes and is recalibrated before real format testing.
Public discovery schema, adverse parsing/config cases, actual formatted/unformatted/
invalid source, unchanged host source and Resource readback are required evidence.

## Sources
- https://github.com/rust-lang/rustfmt/blob/main/src/cargo-fmt/main.rs
- https://github.com/rust-lang/rustfmt/blob/main/src/emitter/diff.rs
- https://github.com/rust-lang/rustfmt/blob/main/src/rustfmt_diff.rs
These upstream sources guide grammar; pinned runtime tests are authoritative.
