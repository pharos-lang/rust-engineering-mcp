# M1-05 external review

Claude Code2.1.259, explicit claude-sonnet-5, medium, tools disabled/read-only safe/restricted. Actual modelUsage verified; Haiku bookkeeping is not reviewer. Packet50607bytes. Review before final live evidence, no code edits.

## M1-05 Clippy Review

### P0 — none found
No security or correctness defect that should block merge was identified in the diff/files provided. The typed selection/profile validation reuses `CheckOptions` closed validation (`crates/domain/src/clippy.rs:37-43`), the gateway command is fixed argv with a validated, closed selection appended (`rust_gateway.rs` Phase::arguments), and the MCP contract mirrors the existing check tool's schema/size-limit machinery.

### P1

1. **Diagnostic source classification only trusts the top-level `code` prefix; code-less clippy diagnostics silently mislabel as `Rustc`.**
   `crates/execution-adapter/src/cargo_diagnostics.rs` (parse, ~lines 352-365):
   ```rust
   let source = parent.map_or_else(
       || if raw.code.as_ref().is_some_and(|code| code.code.starts_with("clippy::")) {
           DiagnosticSource::Clippy
       } else { DiagnosticSource::Rustc },
       |index| nodes[index].source,
   );
   ```
   This is correct for the common case, but any top-level clippy-originated message that lacks a `code` (e.g. some `span_lint`-style diagnostics or clippy-internal notes without a stable lint code) defaults to `Rustc`. The added test (`cargo_diagnostics.rs:458-476`) only exercises a coded parent + code-less *child* (which correctly inherits), not a code-less *top-level* clippy diagnostic. Since diagnostics are explicitly documented as "project-writable untrusted output," the classification boundary should be exercised against that case rather than assumed.

2. **Calibration only exercises the `Default` clippy argv path, not the `--` fork used by Strict/Pedantic.**
   `rust_gateway.rs` (`RustGateway::new` phase list):
   ```rust
   Phase::Run(RustCommand::ClippyProject(
       rust_engineering_domain::ClippySelection::default().try_into()...
   )),
   ```
   Startup calibration proves `cargo clippy --frozen --message-format=json --jobs=1` runs cleanly in the approved sandbox, but never calibrates the `-- -D warnings` / `-- -W clippy::pedantic` trailing-arg fork that real Strict/Pedantic calls will use. If the sandbox's argv/env handling behaves differently once a `--` separator and rustc-flag pass-through is present (quoting, arg-count limits, etc.), calibration won't catch it. Given ADR-036 leans on calibration as evidence the approved gateway path works, this gap should be closed (calibrate one non-default profile) or explicitly called out as a known limitation.

3. **Clippy reuses `check`'s execution budget (`ExecutionLimits::new(30_000, 256*1024)`) unchanged, despite clippy additionally running build scripts/proc macros and a full lint pass.**
   `project_inspection.rs` (`cargo_validation`, shared with `check`). This isn't necessarily wrong, but it's unverified for the heavier clippy path and is exactly the gap the ADR itself calls out as pending ("Execute the actual build.rs and proc-macro containment fixtures through Clippy as additional evidence" — ADR-036 line 26). Flagging as an evidence gap to close before Done, not a proven defect, consistent with the pending-fixtures status already tracked.

### P2

1. **Argument construction for Clippy duplicates the package/workspace/features/all_targets projection instead of sharing it with `CheckProject`'s equivalent block** (`rust_gateway.rs`, new `if let Self::Run(RustCommand::ClippyProject(options)) = self { ... }` block). The ADR calls for shared "Cargo result normalization," which is respected on the diagnostics side (`cargo_diagnostics.rs`) but not on the argv-building side — two parallel implementations of the same projection risk silent divergence (e.g. feature join/sort semantics) if one is changed without the other. Low risk today since both derive from the same validated `CheckOptions`/`ClippyOptions` internals, but worth a follow-up factor.

2. **No visible test drives `encode_bounded`'s diagnostics-trimming loop (`stdio/clippy.rs:520-551`) with clippy-scale payloads** (e.g. Pedantic profile against a fixture with many multi-span suggestions). Clippy diagnostics are typically larger/more numerous than plain `check` output, so the 512 KiB fallback path deserves its own regression test rather than relying on check's existing coverage of the shared macro-level logic.

3. `LintProfile::Default` and `LintProfile::Project` are intentionally argv-identical (ADR-036 line 14) — confirmed by-design, not a defect, noted only so it isn't mistaken for dead code during review.

### Not flagged as defects (explicit evidence gaps / by-design, per project tracking)
- Missing real end-to-end MCP snapshot content for `crates/mcp-server/tests/snapshots/clippy-tool.json` was not reviewable here; per the stated pending status this is expected before Done, not a code defect.
- Hostile build.rs/proc-macro containment fixtures through the actual Clippy path are explicitly tracked as in-progress; not treated as a finding against current code.
- Source-level lint suppression (`#[allow(...)]`) affecting Strict/Pedantic results is by design (ADR-036 line 18) and correctly documented in the tool description (`stdio/clippy.rs:456`).
