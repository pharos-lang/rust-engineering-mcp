# W07 — informe del worker (Opus)

| Campo | Valor |
| --- | --- |
| Modelo | Claude Opus 5 (`claude -p --model opus --effort high --disallowedTools Agent Task`) |
| Inicio / fin (UTC) | 2026-09-12T18:44:44Z / 2026-09-12T19:56:04Z; 4279003 ms |
| Resultado | `subtype: success`, `is_error: False` |
| Transcript | fuera del árbol; hashes en `transcripts.sha256` |

Verificación del orquestador: workspace `check`/`clippy`/`fmt` verdes; dominio
analyzer 43, mutation 8/10, application analyzer 29, protocol (33 snapshots),
mutation_digest, G6 — todo verde. Cut nativo **m6-11 ejecutado por el
orquestador**: el binario real ofrece dos acciones aplicables en `let sum`
(`refactor_inline` "Inline variable" y una de introducir constante), ambas
aplican limpiamente y re-resuelven por digest; recibo `cut-m6-11-code-actions.json`.

## Informe literal del worker

# W07 report: code action → M2 `MutationCandidate` (M6-04)

## Task
Domain, gateway and application support for turning a resolved rust-analyzer code action into an M2 mutation candidate, plus the analyzer provenance decoder (D4). No MCP tool and no commit. Docker tests were not run.

## Result
D1–D4 are implemented. Workspace `check`, `fmt --check`, `clippy -D warnings`, the architecture gate and the docs link check all pass. The native `m6-11-code-actions` cut is written but was not run, so I have no evidence yet of which actions the real binary offers.

Two parts of the spec didn't fit as written. I resolved both toward the stronger constraint (decisions 1 and 2 below).

## Files changed
- **Domain**
  - `crates/domain/src/mutation.rs`: new `AnalyzerActionApply` kind.
  - `crates/domain/src/analyzer.rs`:
    - `AnalyzerError` gains `FileNotInSnapshot` and `NoEdits`.
    - `AnalyzerAction::digest_input`, `apply_action_to_bundle`, `summarize_edits` and `EditsSummary`.
- **Gateway**
  - `crates/execution-adapter/src/analyzer_gateway.rs`: `action_digest`, `action_digests`, `resolve_action_candidate` (match by digest in a fresh, unfiltered session), a session fingerprint and a structural re-check.
  - `crates/execution-adapter/src/project_inspection.rs`: implements the two new port methods.
  - `crates/execution-adapter/src/analyzer_native.rs`: the `m6-11-code-actions` cut, with the new fixture `fixtures/analyzer-actions/`.
- **Application**
  - `crates/application/src/analyzer.rs`: port extension, `analyzer_actions`, `analyzer_action_candidate`, `ActionCandidateError`.
  - `analyzer_finish` was split into a shared report helper; behaviour is unchanged.
- **New match arms required by the new kind**
  - Writer digest: `crates/project-adapter/src/mutation_store.rs`.
  - Journal operation name and scope (same rule as fmt/fix): `crates/project-adapter/src/filesystem/macos/mutation.rs`.
  - `crates/application/src/resolution.rs`.
  - `preview_diff` in `crates/mcp-server/src/stdio/mutation.rs`.
- **mcp-server (D4):** `analyzer_action_validation_view` in `crates/mcp-server/src/stdio/mutation.rs`.
- **Tests:** `crates/project-adapter/tests/mutation_digest.rs` and the G6 test in `crates/project-adapter/tests/support/native_mutation.rs`.

## Tests executed
| Command | Result |
|---|---|
| domain `analyzer` | 43 passed (8 new) |
| domain `mutation` | 10 passed |
| execution `--lib analyzer_gateway` | 28 passed (5 new) |
| execution `--lib lsp_codec` | 54 passed |
| application `analyzer` | 29 passed (11 new) |
| mcp `--test protocol --no-run` | builds |
| mcp `protocol modern_discovery_and_deterministic_project_tool` | 1 passed: all 33 tool schemas match their snapshots, including the 5 M2 ones |
| mcp `--bins provenance` | 7 passed (1 new) |
| project `--lib unknown_or_foreign_kind` (G6) | 1 passed (new) |
| project `--test mutation_digest` | 3 passed |
| application `--test resolution` | 12 passed |

The full-workspace clippy run came before my last edit to the G6 test. I re-ran clippy for project-adapter and `fmt --check` afterwards; both are clean.

## Evidence
- **Native cut not run**, so no elicited actions are recorded. When run, the cut:
  - records every candidate's title, kind, edits and rejection reason;
  - fails with that record if no applicable action applies cleanly;
  - then checks that a second fresh session resolves the same action by its digest.
- **The M2 contract is unchanged:** the discovery test above compares the live `tools/list` against every snapshot.
- **G6:**
  - An unknown kind is refused as `RecoveryRequired` by both a `FormatApply` store and an `AnalyzerActionApply` store.
  - A real analyzer journal read by a `FormatApply` store is `PermissionDenied`.
  - Nothing changes on disk in any of these cases.
  - Relabelling a journal's kind is also refused (`RecoveryRequired`), because the digest covers the kind.

## Decisions
1. **D4 adds no `ValidationMethod` variant.** That enum is pinned in `fmt-apply-tool.json` and the other four M2 snapshots, so a new variant would change all five published contracts. Instead there is a separate `AnalyzerActionValidationView` (method `workspace_edit_structural_only`), and `validation_view` still refuses `m6-analyzer-action-v1`. W08 should publish the new view from its own schema.
2. **`before` is the complete fresh capture, not only the touched files.** This mirrors fmt: `finish_manifest_preview` compares the whole capture, and the writer requires `before` and `after` to have the same shape.
3. **Validation field 9 is the analyzed snapshot's fingerprint** (before the edits). Nothing ran against `after`, and the application layer cannot hash. The decoder names it `analyzed_source_fingerprint`. The tail carries the analyzer version, `binary_sha256`, `config_digest` and `action_digest`.
4. **Signature changes from the spec:**
   - `analyzer_action_candidate` also takes a `MutationPublisher` and authorizes before capturing, as `prepare_format` does.
   - Errors use a new `ActionCandidateError`, because the mcp-server analyzer tool matches `AnalyzerRequestError` exhaustively.
   - Both port methods take `limits`.
   - `action_digest` takes `AnalyzerRuntime` and returns a `Result`.
   - There is no `identity_expectations` parameter: the digest already covers the runtime.
5. **What the digest covers:** title, kind and the sorted edits. It excludes `is_preferred`, so a flipped hint does not make a previewed action stale.
6. **The apply-preview session sends no `only` filter,** since the digest does not depend on it.

## Risks
- The fixture and cursor position (`let sum`, line 2 column 9) are my guess at where assists appear; the real binary may offer nothing there.
- Preview only sees the first 32 actions. An action that was visible in a filtered listing could fall past that cap in the unfiltered preview and be reported as stale.
- As decided by the owner, an applied action is not compile-verified.

## Open issues
- I did not check whether `RustGateway::configuration_fingerprint` covers the analyzer phase's arguments; the session fingerprint relies on it.
- A rejected action carries only its reason in the domain, so W08 cannot publish the title and kind that ADR-083 §4 lists for every action.
- `fixtures/analyzer-actions` is not registered in `fixtures/README.md` or `corpus-sha256.json` (`analyzer-references` isn't either).
- ADR-083 and the M6 matrix are not updated for decisions 1–3.
