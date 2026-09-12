# W08 — M6-04/05: the `rust.analyzer.actions` and `rust.analyzer.action.apply` MCP tools (via the single M2 writer)

Model requested: Claude Opus 5 (`claude -p --model opus --effort high`). Role: implementation worker for the analyzer write path — the second-most security-critical boundary of M6. Orchestrator: Claude Fable 5.1 (running as Opus 4.8). You may not spawn subagents. **Never run a command in the background.** Do not run Docker-backed tests (the orchestrator runs the native suite). Any `--root` you pass a test must be canonicalized. Do not commit. You build directly on the **uncommitted working tree** produced by W07 (its report is `docs/validation/M6/delegation/W07-action-candidate/report.md`); W07 is not committed — it ships with your work as the M6-04/05 vertical.

## Read first

`AGENTS.md`; `docs/adr/ADR-083-analyzer-contract-and-actions.md` §1.2, §4, §5, §6; the W07 code you consume: `crates/application/src/analyzer.rs` (`analyzer_actions`, `analyzer_action_candidate`, `ActionCandidateError`, the `AnalyzerPort` action methods), `crates/execution-adapter/src/analyzer_gateway.rs` (`action_digest`, `resolve_action_candidate`), `crates/domain/src/analyzer.rs` (`AnalyzerAction`, `EditsSummary`, `apply_action_to_bundle`), `crates/mcp-server/src/stdio/mutation.rs` (the ENTIRE preview→commit→receipt machinery you reuse: the `MutationInput` trait, the `Action`/`FormatAction` enums, `MutationPlans::remember`/`resolve`, `commit_mutation`/`replay_mutation`/`mutation_receipt`, `preview_diff`, `mutation_digest`, `validation_view` and the new `analyzer_action_validation_view`/`AnalyzerActionValidationView` W07 added, the `Preview`/`Receipt` `Data`, and the host write-grant flags `--allow-*-write`); `crates/mcp-server/src/stdio/analyzer.rs` (the existing analyzer read tools' shape you mirror for the read side); `crates/mcp-server/src/host_config.rs` (add the new grant); `crates/mcp-server/tests/protocol.rs` (snapshot list, `assert_output`); `scripts/release-smoke.py`. The independent review whose P3s you must close: `docs/validation/M6/delegation/V07-review-action-candidate/disposition.md`.

## Owner decision A (binding)

`rust.analyzer.action.apply` validates only structurally (existing-file non-overlapping bounded TextEdits, matching version, no Command/snippet/resource-op/external-URI — already enforced by the codec + `apply_action_to_bundle`) plus the exact preview diff plus the plan bound to analyzer/config/source identity. **No cargo check.** The tool description and `docs/security-model.md` must state plainly that an applied code action is **not compile-verified** and the caller should run `rust.check` after.

## Deliverables

### D1 — `rust.analyzer.actions` (tool 35, read-only)

New tool (in `crates/mcp-server/src/stdio/analyzer.rs` or a sibling module) `{project_ref, expected_project_fingerprint?, file, range{start,end}, only: [kind]?, timeout_seconds}` → the ADR-083 §3 envelope with `actions: [{action_digest, title, kind, is_preferred, applicability: applicable | rejected {reason}, edits_summary: {files, edits, bytes_delta}}]`, `omitted`. Annotations readOnly/idempotent/non-destructive/closed-world. **V07 P3: rejected actions must still carry a title and kind where ADR-083 §4 asks** — if the domain `ActionCandidate::Rejected` only carries the reason, extend the domain/gateway additively so a rejected action can publish its title/kind (or document precisely why it cannot and publish `{applicability: rejected {reason}}` without them, noting the limit). Bootstrap refusal, `Conflict`, `FileNotInSnapshot`, timeouts/never-ready/crash/limits map exactly as the other analyzer tools. No analyzer stderr/message text on the wire.

### D2 — `rust.analyzer.action.apply` (tool 36, write via the M2 writer)

A `MutationInput`-style tool with the three modes, reusing the M2 writer exactly like `rust.fmt.apply`:
- `preview {expected_project_fingerprint, action_digest, file, range}` → calls `analyzer_action_candidate` (W07) to build the `MutationCandidate{kind: AnalyzerActionApply, …}`, then feeds it through the SAME `preview_diff`/`mutation_digest`/`MutationPlans::remember` path the M2 tools use, returning the `Preview` `Data` {plan_id (`mut_…`), plan_digest, expires_in_seconds, files, diff, validation: the `AnalyzerActionValidationView`}. **V07 P3-2: make the touched-file list prominent** — the preview already returns `files`/`diff`; ensure the description and the `files` field foreground that an action may rewrite multiple captured files, and that the caller must review the diff.
- `commit {plan_id, plan_digest, idempotency_key}` → `commit_mutation` through the existing `MutationPublisher`/`NativeMutationStore` (the single writer), with generation/authority/idempotency exactly as fmt.apply; invalidates the project_ref.
- `receipt {operation_id, recover}` → `mutation_receipt`.
- Host grant: add `--allow-analyzer-action-write <root>` in `crates/mcp-server/src/host_config.rs` (same family as `--allow-fmt-write`); the tool is `unavailable/SANDBOX_DENIED` without it. Description states: not compile-verified; local_coordinated (no OS exclusion / multi-file atomicity); commit invalidates the project_ref; the action digest binds analyzer/config/source so a runtime rollback or a changed capture makes the plan stale.
- **V07 P3-3: add a cross-crate encode→decode round-trip test** — encode a candidate's `validation` in the application/gateway path and decode it with `analyzer_action_validation_view`, asserting every field survives; a field reorder on either side must fail it.
- **V07 P3-4: the decoded `AnalyzerActionValidationView` is used only for the `AnalyzerActionApply` kind** — the wire layer must not accept an analyzer view for a non-analyzer plan or vice versa (the plan digest already binds both; add the cheap kind check and a test).
- **V07 P3-6: move the early `analyzer_source_fingerprint` fallible step inside the quarantine scope** in `project_inspection.rs` so its failure quarantines like the gateway path (small, W07-owned file — you may touch it for this).

### D3 — Registration, snapshots, wire/protocol/mapping tests

Register both tools in `stdio.rs` (fields, constructor, `list_tools`, `call_tool`); snapshots `analyzer-actions-tool.json` (index 34) and `analyzer-action-apply-tool.json` (index 35) in `tests/protocol.rs`; **the 34 existing snapshots stay byte-identical** (assert). Wire tests in the five MCP versions; invalid-argument tests; the full status/code mapping table for both tools; a no-peer-text test; bootstrap tests; `RESULT_LIMIT` trim; and a `preview→commit→receipt` protocol test that (with a fake/mock write path where Docker is unavailable) exercises the plan lifecycle and the project_ref invalidation, plus the `ACTION_STALE` path (a plan whose digest no longer matches). `release-smoke.py`/`test-release-smoke.py` → 36 tools, two new hashes, the other 34 unchanged (assert; stop and report otherwise). The three `tools.len()` regression tests → 36.

### D4 — Native end-to-end (`crates/mcp-server/tests/analyzer_runtime.rs`, `#[ignore]`) — V07 P3-9

Two ignored tests on `fixtures/analyzer-actions` through the **real** server + M6 image: (a) `rust.analyzer.actions` lists ≥1 applicable action with a digest; (b) `rust.analyzer.action.apply` preview on that digest returns a diff, commit through the writer actually changes `src/lib.rs` on disk to the edited bytes, the receipt reflects it, and the pre-commit project_ref is invalidated (a second call on it fails). Also an `ACTION_STALE` native case if feasible (mutate the file between preview and commit). `scripts/test-m6-runtime.py` must discover them (it discovers `analyzer_runtime.rs`). Do not run them — the orchestrator runs the suite. Add a native cut in `analyzer_native.rs` only if the gateway needs one beyond m6-11; otherwise the e2e tests suffice.

### D5 — Docs (same commit): `docs/tools.md` (both contracts in the M6 section; the not-compile-verified property; the touched-file/diff-review guidance; inventory 36), `README.md` (36 tools; one paragraph on analyzer write actions and the `--allow-analyzer-action-write` grant), `CHANGELOG.md`, `docs/security-model.md` (the analyzer write path: hostile-influenced edits, structural-only validation, not compile-verified, the diff is the review surface, single M2 writer, up-to-128-file scope), `docs/compatibility.md` (36), `docs/client-configuration.md` (the new grant), `docs/validation/M6/matrix.md` (M6-04 and M6-05 rows), `docs/implementation-status.md`, ADR-083 (a dated note for W07's decisions 1-3 if not already recorded: separate `AnalyzerActionValidationView`/`workspace_edit_structural_only`, whole-bundle before, field-9 = analyzed source fingerprint).

## Verification (targeted, foreground)

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-domain --locked --offline analyzer
cargo test -p rust-engineering-application --locked --offline analyzer
cargo test -p rust-engineering-mcp --locked --offline analyzer
cargo test -p rust-engineering-mcp --locked --offline --test protocol
cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run
cargo test -p rust-engineering-mcp --locked --offline --test catalog_status --test crate_inspect --test crate_search
python3 -B scripts/check-architecture.py
python3 -B scripts/test-release-smoke.py
python3 -B scripts/docs-hygiene.py links-check
```

## Constraints

Reuse the single M2 writer — no second writer/journal, no bypass of `MutationPublisher`'s authorize/generation/idempotency. Domain/application free of serde_json/rmcp/process/sha2. No new dependencies. No `unwrap`/`expect`/`panic!`/`unsafe` outside `#[cfg(test)]`. Do not change the observable behaviour or snapshots of the 34 existing tools or the M2 tools. If a V07 P3 cannot be closed without changing an M2 snapshot or a frozen contract, STOP and report. Report: Task / Result / Files changed / Tests executed (counts) / Evidence (two new snapshot hashes; how each V07 P3 was closed) / Risks / Decisions / Open issues.
