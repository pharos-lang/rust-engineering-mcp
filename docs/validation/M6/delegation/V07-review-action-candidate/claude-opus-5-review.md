# Review: W07 / M6-04 analyzer action → `MutationCandidate`

## Verdict

**Approve with P3.**

I found no P0–P2 issues. The digest is injective and order-independent. Edits are fail-closed at the adapter and again at the domain. Every `MutationKind` match visible in the diff is updated. There is still exactly one writer. The P3s below are mostly places where a safety property holds only because one layer enforces it, or because two crates are kept in sync by hand. W08 should close those rather than inherit them.

## Findings

| Sev | File / function | Finding | Evidence from the diff |
|---|---|---|---|
| P3 | `domain/src/analyzer.rs` `apply_action_to_bundle` / `apply_edits`; `analyzer_gateway.rs` `structural_rejection` | **The effect is order-independent only because the adapter rejects coincident starts.** `digest_input` sorts edits, so two orderings share one digest. Two zero-width inserts at the same position are the one case where LSP semantics make order matter. They are rejected by `structural_rejection` (`pair[0].1 == pair[1].1`, tested by `shared_start`). No domain test shows that `apply_edits` rejects them too. The doc on `apply_action_to_bundle` promises "input order is irrelevant" regardless. If `apply_edits` accepts coincident inserts, the domain function breaks its own postcondition, and the guarantee rests on one adapter-layer check. The listing path (`resolve_actions` → `action_digests`) also never runs `structural_rejection`. The listing can therefore publish an `Applicable` digest that apply-preview then returns as `Rejected`, if the codec is weaker than this check. | domain tests cover only true overlap (`(1,1,1,4)`/`(1,3,1,5)`). `too_many` uses identical inserts at `(1,1)` but hits the count check first, so it proves nothing about coincident inserts. |
| P3 | `application/src/analyzer.rs` `analyzer_action_candidate` scope loop; `macos/mutation.rs` `candidate_files` | **Scope is borrowed from rustfmt/cargo-fix, but these edits are free-form peer output.** An action requested at one `file`/`range` may rewrite up to 128 captured `.rs` files anywhere in the bundle. Nothing ties the edits to the requested file, and `build.rs`, proc-macro sources and vendored `.rs` files are all allowed. rustfmt and cargo-fix produce constrained transformations; a code action's `new_text` is arbitrary. The mitigations are the exact diff and the documented not-compile-verified property. This is an owner-policy call, but W08's preview should make the touched-file list prominent. | `if !before.path().ends_with(".rs") { PermissionDenied }`, `changed > 128`; `candidate_files` arm comment: "same closed scope as rustfmt and cargo fix". |
| P3 | `application` `frame_validation` field list vs `mcp-server` `analyzer_action_validation_view` | **No encode→decode round trip exists across the two crates.** The 13-field order is maintained by hand on both sides, and each side's test passes with data the other side would refuse. The application fixture uses `image_id: "sha256:m6"`, but the view requires `image_id.parse::<SourceFingerprint>()`. A field reorder in either crate would not fail any test. | `action_runtime()` → `"sha256:m6"`; view loop `for fingerprint in [&image_id, …]`. |
| P3 | `project-adapter` writer; `native_mutation.rs` foreign-kind test | **The writer does not tie the `validation` version to the kind.** The new test itself journals an `AnalyzerActionApply` candidate carrying an `m2-fmt-apply-v1` (compile-verified) validation string, and it reaches `Prepared`. Kind/provenance consistency therefore depends only on `analyzer_action_candidate` building the string and W08 choosing the right view. `validation_view` does refuse `m6-analyzer-action-v1` (tested), but the reverse mislabel is not refused anywhere below the application. | `candidate = format_request.candidate; candidate.kind = MutationKind::AnalyzerActionApply;` then `commit_checked` reaches `CommitCheckpoint::Prepared`. |
| P3 | `application` `analyzer_action_candidate` | **The prelude is re-implemented instead of shared.** `analyzer_actions` uses `analyzer_prelude`. The candidate path hand-rolls the same steps: `resolve_inner`, the fingerprint conflict check, `authorize`, `source_inner`, `captures_file`, `range_outside`. The different order is intentional, since authorize must run before capture. But any extra check inside `analyzer_prelude` (not shown in this diff) is silently missing from the mutation path. Parity needs verifying. | `let identity = self.resolve_inner(reference, control, false)?; … publisher.authorize(…)?; let source = self.source_inner(…)?;` |
| P3 | `project_inspection.rs` `resolve_actions` / `resolve_action_candidate` | **Some `Internal` errors skip quarantine.** `analyzer_source_fingerprint(source)?` returns early, before `with_gateway` and `quarantine_if_uncertain`. Its `.parse()` failure produces `InspectionError::Internal` without quarantining. The same error from inside the gateway closure does quarantine. The impact is low, but it contradicts the rule stated on `quarantine_if_uncertain`. | `let source_fingerprint = analyzer_source_fingerprint(source)?;` precedes `let result = self.with_gateway(…); self.quarantine_if_uncertain(&result);` |
| P3 | `analyzer_gateway.rs` `resolve_action_candidate`; mcp view | **Some provenance fields are constants, not observations.** `platform` is the constant `"linux/aarch64"`, and `rust_version`/`cargo_version` come from `APPROVED_*` constants rather than the session. The decoder never checks `platform`, `rust_version` or `cargo_version`, so empty strings decode. `image_id` binds these indirectly. This is acceptable if `m2-fmt-apply-v1` does the same, but the view presents them as run facts. | `platform: ANALYZER_PLATFORM.to_owned()`, `rust_version: super::rust_gateway::APPROVED_RUST_VERSION`; the view has no checks on those three frames. |
| P3 | `domain` `digest_input`; `analyzer_gateway.rs` `action_digest` doc | **Two assumptions about the digest need stating or verifying.** (a) The `kind` encoding is only injective if `CodeActionKind::to_lsp` maps every variant to a distinct string; that function is not in the diff. (b) The `action_digest` doc says a new "configuration" changes the digest. It binds the analyzer `config_digest` but not `image_id` or the gateway `configuration_fingerprint`. This is safe because the edits themselves are bound, but the doc overstates what is covered. | `push_field(&mut out, kind.to_lsp().as_bytes())`; `action_digest` pushes `version`, `binary_sha256`, `config_digest` and `source_fingerprint` only. |
| P3 | `analyzer_native.rs` cut `m6-11` | **The native cut is sound but narrow.** It does not pass vacuously: it fails if no action is usable (`cut.fail(…)`) and asserts `Resolved`. It proves two things: a real applicable action changes `after`, and a second session re-resolves it by digest. It does not exercise the production port (`RustProjectInspector::resolve_*`), `analyzer_action_candidate`, the scope check or the validation framing. It computes the source fingerprint with its own copy of encode+digest instead of `analyzer_source_fingerprint`. There is no native `Stale` case (a changed byte between the two sessions). `same_action` re-checks what `Resolved` already implies. **The recorded gap (no Command/snippet/resource-op rejection elicited) is not covered by this diff.** The gateway tests only construct `ActionRejection::Command`/`Snippet` values; they never exercise the codec producing them. Coverage must come from the W06 `lsp_codec` tests, which should be confirmed to cover Command, snippet, resource ops, external URI and version mismatch. | `digest(&source_archive::encode(&source)…)` inline; `rejections` recorded, not asserted; the test-only `answered_actions(vec![…Rejected(Command)…])`. |

## Per-goal conclusions

### 1. Digest soundness

No blocking finding.

- **Encoding.** `digest_input` is a sound encoding:
  - It starts with a domain prefix.
  - Every variable field (title, lsp kind, file, `new_text`) has a u64 length prefix.
  - Positions are fixed-width `u32` pairs, and `kind` carries a 0/1 tag.
  - The edit count is prefixed.
  - Edits are sorted on the complete tuple `(file, start, end, new_text)`, so edits that compare equal are byte-identical and ordering cannot change the bytes.
- **Field boundaries.** `action_digest` length-prefixes each component again, so boundaries cannot be forged.
- **Replay.** A plan cannot be replayed across analyzer version, binary or config, or across a changed capture. The whole-bundle `source_fingerprint` is conservative: any change to any file makes the plan stale.
- **`is_preferred` exclusion.** Safe, because it does not affect what gets written.
- **`only` exclusion.** Safe, because resolution runs unfiltered and matches on the edits themselves, not on membership in a list.
- **Dangerous action matching a benign digest.** This would need a sha256 collision.
- **Title.** The title is peer-controlled but bound into the digest, so it cannot be swapped independently of the edits. The UI must still treat it as untrusted.

### 2. `apply_action_to_bundle`

See P3-1 on coincident inserts. Otherwise sound:

- Empty edit set → `NoEdits`; more than `MAX_EDITS` → `LimitExceeded`.
- A file absent from `before` → `FileNotInSnapshot`. The binary search fails closed if the sort assumption is ever wrong.
- Edits are grouped per path, so overlap is checked across all edits to the same file regardless of their order in the input.
- File and bundle limits are enforced by `SourceFile::new` and `SourceBundle::with_directories`, mapped to `LimitExceeded`.
- Untouched files and all directories are cloned unchanged, and file count and order are preserved.
- The application re-checks path alignment (`Invalid`).
- All errors are typed. I saw no `unwrap` or `panic` on the non-test path; `pair[0]` inside `windows(2)` is bounds-safe.
- A non-UTF-8 source fails in `LineIndex::new`. UTF-8 safety of the result depends on `byte_offset_from_position` never splitting a code point. That function is not in the diff; worth confirming.

### 3. Single writer / G6

No finding beyond P3-4.

- **Match arms updated:** `mutation_digest`, `operation_kind`/`operation_name`, `candidate_files`, both matches in `resolution.rs` (fail-closed `Invalid`/`false`), and `preview_diff`.
- **Other exhaustive matches** are enforced by the compiler; only wildcard `_` arms elsewhere could hide a missed case.
- **Unknown kinds** are refused with `RecoveryRequired` before any effect, with no project change, for both a FormatApply reader and an AnalyzerActionApply reader.
- **Relabelling** a journal breaks the kind-bound digest.
- **Foreign kinds** are refused with `PermissionDenied`.
- **No second writer or journal** is introduced.
- **Authorization:** `authorize` runs before capture and before the port (tested: `calls == 0`).
- **Generation and idempotency** are commit-time checks, so they cannot be assessed until W08 wires `MutationPlans`.

### 4. `validation` encoding

- **Framing.** It uses the same `{len}:{bytes}` framing as the M2 encodings.
- **Decoding.** The view reads exactly 13 frames and requires the exact version, `local_coordinated` mode and no trailing bytes. It parses 7 fingerprints and requires a non-empty analyzer version. Truncated, extended and fmt-shaped inputs are refused (tested).
- **Frozen schemas.** `ValidationMethod` is untouched, so the 5 frozen M2 schemas are unaffected, and `validation_view` still refuses the new version.
- **Field 9.** It is named `analyzed_source_fingerprint`. Nothing in the diff treats it as the after fingerprint. W08 should compare it against the digest of `before`, never `after`.
- See P3-3, P3-4 and P3-7.

### 5. Architecture

No finding beyond P3-5 and P3-6.

- **Domain** adds only std `BTreeMap` and `Serialize` on `EditsSummary`, with no sha2 or serde_json; hashing stays in the adapter.
- **Application** has no serde_json, rmcp or process dependencies.
- **Refactor.** `call_limits` and `analyzer_report` are pure extractions: `analyze` now calls them with the same arguments.
- **Existing tests** are only extended: the stub port gains the new trait methods and `mutation_digest.rs` adds one kind to its loop. No snapshots changed.

### 6. Native cut

See P3-9. The cut proves the two M6-04 properties on the real binary. The unsafe-form rejection gap depends on the W06 codec tests, not this diff.

## What I verified

- **Digest encoding:** byte-level injectivity of `digest_input` (prefixes, tags, fixed widths, count) and its order-independence under a total sort key; length-prefixing of every `action_digest` component.
- **Staleness:** the listing digest uses the listing capture's fingerprint and the preview recomputes it over a fresh capture (`analyzer_source_fingerprint` in both port methods), so any byte change yields `Stale`.
- **`structural_rejection` overlap logic:** checking adjacent pairs after sorting on `(file, start, end)` is complete for overlaps. Touching edits (`end == next.start`) are allowed and unambiguous; equal starts are rejected.
- **Every error path** in `edits_by_file`, `summarize_edits` (checked add; the subtraction cannot overflow) and `apply_action_to_bundle`.
- **Application check order:** conflict → authorize → capture → file/range → port → post-session revalidation (`analyzer_report` → `resolve_inner(…, true)`) → resolution → `image_id` consistency → apply → cancellation check → scope → framing. Tests cover denial-before-port, conflict-before-port, stale, rejected, not-answered, not-applicable (range, missing file, overlap) and no-change (identity edit, empty edits).
- **Listing alignment:** `summarize_actions` refuses digest/candidate misalignment (length mismatch, or `Applicable` without `Some` and vice versa) and never publishes a digest for an action it cannot summarize.
- **New `MutationKind` arms** in all six match sites in the diff, and the journal refusal behaviour in the new native test (unknown, relabelled, foreign kind; journal bytes and project tree unchanged).
- **View tests:** the decoder refuses each listed malformed field, bad frame count and trailing bytes, and `validation_view` refuses the analyzer encoding.
- **Not verifiable from this diff:** `apply_edits` handling of coincident inserts, `CodeActionKind::to_lsp` injectivity, `LineIndex` code-point boundaries, the body of `analyzer_prelude`, whether `mutation_digest` covers `validation`, and the W06 codec's unsafe-form rejection tests.