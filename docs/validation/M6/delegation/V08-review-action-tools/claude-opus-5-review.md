# Review: W08 analyzer write path (`rust.analyzer.actions`, `rust.analyzer.action.apply`)

## Verdict

**Approve with P3.**

- **No bypass found.** I found no second writer, no second journal, and no way to skip authorize, generation or idempotency.
- **Staleness binding holds.** A plan cannot be committed after the source under it changes.
- **The M2 audit refactor is behaviour-preserving.**
- **What remains:**
  - error codes that don't match across modes;
  - a kind check that runs later than it should;
  - small gaps between what the listing marks applicable and what preview accepts;
  - test gaps that leave a few goal-5/7 properties unproven.

Citations are by function. The new files have no stable line anchors in the diff, and I did not want to give wrong line numbers.

## Findings

| Sev | File / function | Finding | Evidence |
|---|---|---|---|
| P3 | `mutation/analyzer_action.rs` `Ports::commit` | **Wrong-kind check runs after `plans.resolve`.** If `resolve` binds the idempotency key or marks approval (the Conflict message "idempotency key or approval conflicts" suggests it does), then a commit through the analyzer tool with an M2 plan's id and digest binds a key to that foreign plan before the PERMISSION_DENIED refusal. A later `rust.fmt.apply` commit with its own key would then CONFLICT. This is cross-tool interference within one client, not a write. | `let resolved = shared.plans.resolve(&id, plan_digest, key.clone(), …)` comes first; `if plan.request.candidate.kind != AnalyzerActionApply` is checked afterwards. |
| P3 | `Ports::commit` / M2 `MutationTool` commit (not in diff) | **The reverse direction (analyzer plan committed through an M2 tool) is not evidenced.** The only wrong-kind unit test puts a `FormatApply` plan through the analyzer tool. The native-store test shows a FormatApply store refusing an analyzer journal on `receipt`/`recover`/`replay`, but not on `commit`. The plans are shared (`Arc::clone(&mutation_plans)`), so an analyzer plan is reachable from the five M2 commit paths. Please confirm an M2-side `kind != I::KIND` guard or a store-level commit kind gate exists. | `a_plan_of_another_writer_kind_is_refused_before_the_writer` covers one direction only. `a_journal_naming_an_unknown_or_foreign_kind…` never calls `commit` on a store opened for another kind. |
| P3 | `analyzer_action.rs` `preparation_failure`, `request_failure`, `joined_output` | **One condition, several codes (goal 5).** A stale or invalidated `project_ref` gives PROJECT_NOT_FOUND on preview (`inspection_failure`). The same ref gives PERMISSION_DENIED on commit (`Rejected(_) → PermissionDenied`) and on receipt (the unit test asserts this). A source change caught before the writer is ACTION_STALE. The same change caught by the writer is CONFLICT (`mutation_failure`), whose message says "digest, idempotency key or approval". Any `Aborted` receipt, including a replay or receipt of an op aborted for cancellation or I/O, is ACTION_STALE with the `SOURCE_CHANGED` message. `InvalidProject` from `finish_manifest_preview` is also ACTION_STALE. | `preparation_failure` match arms; `Ok(data @ Receipt{state: Aborted}) => … Some(SOURCE_CHANGED)`; unit test `stale.map(|_| ()) == Err(PermissionDenied)`. |
| P3 | `analyzer_action.rs` `admitted` | **Audit `admitted=false` for every PERMISSION_DENIED.** That includes denials after the grant was present, the store opened and authorize passed: stale ref at commit or receipt, a non-`.rs` scope refusal from the application, and the wrong-kind plan. The event under-reports admission for those calls. | `!(output.error_code == Some(ApplyCode::PermissionDenied) || …)` |
| P3 | `AnalyzerActionApplyTool::call` / `open_store` | **The grant check runs inside the worker.** It comes after `workers.run_joined` admission and `provider.try_lock()`. A call without the grant can therefore surface LOCK_BUSY, CANCELLED or TIMEOUT_TOTAL instead of `unavailable/SANDBOX_DENIED`, which weakens "every call". No plan or store state is created, so the state half of the goal holds. | `let mut provider = provider.try_lock().map_err(lock_failure)?; open_store(&mut provider)?;` sits inside the worker closure. Only the bootstrap refusal returns before the worker. |
| P3 | `worker_failure` | **Commit or receipt timeouts get the wrong message.** `WorkerError::TimedOut` maps to TIMEOUT_TOTAL (`unavailable`) with "Analyzer call exceeded its total budget" in every mode. For a commit, no analyzer runs, and the message doesn't tell the caller to check the receipt (compare `Io`'s wording). | `WorkerError::TimedOut => ApplyCode::TimeoutTotal.into()`. |
| P3 | `application/analyzer.rs` `listed_edits` vs `analyzer_action_candidate` | **Listing/preview parity gaps.** The listing can mark an action `applicable` that preview then refuses. Three cases: (a) edits that change no bytes (preview returns `NoChange` → ACTION_REJECTED); (b) a bundle-level limit breach (`validate_action_edits` skips bundle limits by design); (c) a non-`.rs` edited path, if `AnalyzerFile` does not itself enforce `.rs` (preview returns PERMISSION_DENIED with a grant-wording message). This contradicts the "Never offered as applicable when the preview… would refuse it (V07)" comment. | `listed_edits` has no no-op, bundle or `.rs` check. The `changed == 0` and `ends_with(".rs")` checks exist only in the candidate path. |
| P3 | `stdio/analyzer/actions.rs` `bounded_title` | **Some peer text is not neutralised.** `char::is_control` replaces only Cc characters. Bidi overrides and isolates (U+202A–202E, U+2066–2069), zero-width characters and U+2028/2029 reach the wire in peer-controlled titles on both applicable and rejected actions. The ≤256-scalar bound itself is correct and matches `maxLength` (code points). | `title.push(if ch.is_control() { '\u{fffd}' } else { ch })`. |
| P3 | `ApplyData::Preview.files` | **Touched-file prominence is text-only.** It is carried by the description, the summary and a schema doc comment. Nothing in the data marks entries other than the requested `file`, or `build.rs` and vendored paths. A caller told to "call rust.check after commit" will run a build script the analyzer action rewrote (sandboxed, but still the step the tool recommends). The listing does not expose touched paths at all, only `edits_summary.files`. | `files: Vec<Change>` holds only path, hashes and byte counts. |
| P3 | `domain/analyzer.rs` `validate_action_edits` via `summarize_actions` | **Peer-driven CPU amplification under the registry lock.** Each of up to 32 actions copies every touched file (`apply_edits`) and builds up to two `LineIndex`es per file (`summarize_edits` rebuilds them). Worst case is about 32 × the bundle byte limit of copying per listing, triggered by hostile edits. It is bounded, but not trivially. | `for … edits_by_file(…).values() { apply_edits(…) }`, then `summarize_edits(before, edits)` walks the same files again. |
| P3 | `mutation/audit.rs` `Record` / `ApplyCode::event` | **Audit vocabulary extended without a version bump.** The fixed audit event (same `SCHEMA` constant) now carries `reason` values outside the M2 set (`action_stale`, `action_rejected`, `analyzer_*`, `timeout_*`, `file_not_in_snapshot`…) and a new `tool` value. If the event schema is documented as a closed enum, this changes it silently. | `reason: self.error_code.map(ApplyCode::event)` goes into the same `Event`. |
| P3 | `mutation.rs` `analyzer_action_validation_view`; application tests | **`image_id` format is not tied to the decoder by any non-ignored test.** The decoder requires `image_id` to parse as `sha256:<64hex>`. The application fixture uses `"sha256:m6"`, which the decoder would reject, and the wire `Analyzer.image_id` schema is only `minLength 1`. If the production `gateway.image_id()` form ever differs, every preview fails INVALID_OPERATION after a full session. Only the ignored native test would catch it. The same encoder/decoder test also can't detect a `rust_version`/`cargo_version` swap at the `analyzer_action_candidate` call site, because production and fixtures both use `1.98.1`. | `action_runtime()` has `image_id: "sha256:m6"`; the decoder loop parses `&image_id` as a `SourceFingerprint`. |
| P3 | Tests (goal 7) | **Test gaps.** (a) The fake `Writer::authorize` always succeeds and is not counted, so "authorize once, before the session" is proven only at the application layer (`Grant` + `port.calls == 0`), not in the tool lifecycle. (b) The fake writer doesn't repeat the `before` comparison, so the writer-level stale race is never exercised, only `finish_manifest_preview`. (c) All three lifecycle tests are `#[cfg(target_os = "macos")]`. (d) No tool-level test covers a grant for a different root staying PERMISSION_DENIED. (e) The native `after_text` works only because `preview_diff` emits whole-file hunks (context lines are dropped); a format change would fail loudly, not pass vacuously. | See the `Writer` impl and `cfg` attributes in `analyzer_action.rs` tests, and `after_text` in `analyzer_runtime.rs`. |

## What I verified

**1. Single writer: no finding beyond the kind-check ordering.**
- **Writer.** `writer` is `provider.store` from a `Provider { kind: AnalyzerActionApply }` opened through the same `Provider::store()` M2 uses.
- **Preview.** It authorizes through that store (`publisher.authorize(&entry.project.lease)`) before capture and session. It then uses the M2 `finish_manifest_preview`, `preview_diff`, `mutation_digest` and `remember_revocable` into the shared `MutationPlans`, so the global budgets apply.
- **Commit, replay, receipt.** These call `registry.commit_mutation`, `replay_mutation` and `mutation_receipt` with that store. No other path writes source: the adapter only resolves.
- **Scope.** `.rs`-only and ≤128 files is enforced three times: application, `preview_diff` and native `candidate_files`.
- **Journal.** It uses the existing format with a new `operation_name`. Unknown kinds still map to RecoveryRequired. `mutation_digest` binds the kind label.
- **Invalidation.** It comes from the same `commit_mutation`. The unit test (old ref refused, reopened ref succeeds) and the native test (old ref gives PROJECT_NOT_FOUND) both assert it.

**2. M2 audit refactor: no behavioural finding.**
- **Event fields.** `audit::record(&Output)` computes `status`, `reason`, `duration_ms`, `result_id` and `files_changed` exactly as the removed inline code did. `Event` field order is unchanged.
- **Lost responses.** `AuditedOutput::response_lost` for `Output` matches the old preview→Cancelled rewrite.
- **Validation views.** `receipt_data(receipt, I::KIND)` and `m2_validation_view` refuse only `AnalyzerActionApply`, which no M2 `I::KIND` is. `validation_view` is unchanged and still refuses the analyzer version (tested).
- **Schemas.** `ValidationView`, `ValidationMethod`, `Output` and `Data` are untouched. The new tool reuses `Change`, `ReceiptChange`, `ReceiptState`, `MutationEvidence`, `Truncation`, `Concurrency`, `Freshness`, `SnapshotSemantics` and `Status` without derive or attribute edits. `receipt_state` and `receipt_changes` are pure extractions. The five M2 schemas cannot have moved from this diff.

**3. Staleness and binding: no soundness finding.**
- **Preview.** It revalidates identity, compares the expected fingerprint, authorizes, then takes a fresh `source_inner` capture. The adapter hashes the same bundle into the digest, which covers the action's `digest_input`, analyzer version, binary, config and source fingerprint. `match_action` recomputes the digest, so a new capture gives `Stale` (unit-tested with a different fingerprint).
- **Plan binding.** `before` is that capture, and the plan digest covers `before`, `after`, kind and validation (which carries `image_id`, `configuration_fingerprint` and the session fingerprint).
- **Commit.** `finish_manifest_preview` and `commit_mutation` run under a single registry guard, and the writer re-checks at publication.
- **Contract.** The `plan_digest`/`idempotency_key`/replay contract is fmt.apply's code. Caveats are the resolve-before-kind-check and reverse-direction rows.

**3b. Encode/decode round trip: sound.**
- **Frame order.** Both sides use the same 13-frame order: version, mode, platform, image, config, session, rust, cargo, analyzed, analyzer version, binary, config digest, action digest.
- **Frame format.** Frames are length-prefixed, and trailing bytes or a missing frame fail as Invalid.
- **Reorder detection.** The cross-crate test gives seven distinct hashes and distinct rust/cargo versions, so reordering frames on either side is caught. The residual risks are a call-site swap of equal-valued fields and `image_id` format (see the table).

**4. Information boundary: no leak found.**
- **Apply failures.** Every failure path uses a static `Failure` message. Candidate reports are discarded except for the failure enum. Titles never appear in apply output.
- **Actions tool.** It publishes only `wire_session`, readiness and completeness, as the read tools do. The SECRET kill/reap strings are asserted absent. Rejected titles and kinds go through `NonEmptyText` → `bounded_title` and the closed `CodeActionKind::from_lsp`.
- **Size bounds.** The apply preview is size-checked before the plan is retained (`validate_preview_size`) and again at the waiter, without calling `retain()` on overflow. It refuses rather than trims, which is correct because the exact diff is the review surface. Note that apply has no RESULT_LIMIT code; overflow is `blocked/LIMIT_EXCEEDED`, so check this against ADR-083 §3. The actions listing trims by popping actions, with `omitted` and `result_limit` declared.

**5. Grant and status mapping: mostly verified.**
- **Host config.** `--allow-analyzer-action-write` is parsed into its own roots and must sit under a read root with `--rust`.
- **No grant.** The result is `unavailable/SANDBOX_DENIED`, `admitted=false`, no store opened; the protocol test covers all four phases and events.
- **Other root.** An authorize refusal gives PERMISSION_DENIED (application test).
- **Status set.** `ApplyCode::status` never yields `failed`, and `passed` only accompanies a retained preview or a durable Committed/NoChange receipt.

**6. Contract.** Tools are pushed after diagnostics, so the indices are 34 and 35, matching `protocol.rs`, `release-smoke.py` and the counts updated to 36.
- **Annotations.** Apply: `readOnly=false`, `destructive=true`, `idempotent=false`, `openWorld=false`. Actions: read-only and idempotent.
- **Input contract.** The `only` filter is a closed snake_case enum with `maxItems 7` and dedup. A reversed range gives -32602 in both tools. The preview input has no edit field (tested).
- **Rejections.** The vocabulary is 11 closed reasons, mirrored 1:1.
- **Existing snapshots.** From the diff, none of the 34 existing tools' types changed. `WireRange` and the new schema types are additive and used only by the new tools.

**7. Native e2e: not vacuous.**
- **List test.** It asserts a digest, a title and `files`/`edits` ≥1, and that the listing leaves disk bytes unchanged.
- **Apply test.** It asserts preview leaves disk unchanged. It asserts the commit receipt has `effect_after == intended_after`, and that disk bytes equal the post-edit text rebuilt from the preview diff (`assert_ne!` against the original rules out a no-op). It also checks old-ref invalidation and the receipt under a reopened ref.
- **Stale test.** It requires the preview to pass first. The commit must then be `blocked/ACTION_STALE`, the disk must still hold the external edit, and re-preview of the old digest must be ACTION_STALE.
- **No applicable action.** `first_applicable` errors, so the tests fail rather than pass.
- **Unit lifecycle.** It faithfully models receipt-after-invalidation (real `Registry` and `SecureProjects`), exactly one writer commit, and the wrong-kind refusal. It does not model authorize count or the writer-level re-check (see the table).