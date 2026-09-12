# W08 — informe del worker (Opus)

| Campo | Valor |
| --- | --- |
| Modelo | Claude Opus 5 (`claude -p --model opus --effort high --disallowedTools Agent Task`) |
| Inicio / fin (UTC) | 2026-09-12T20:13:14Z / 2026-09-12T21:33:08Z; 4793393 ms |
| Resultado | `subtype: success`, `is_error: False` |
| Transcript | fuera del árbol; hashes en `transcripts.sha256` |

Verificación del orquestador: workspace `clippy`/`fmt`/arquitectura verdes;
dominio analyzer 45, application 30, gateway/codec 84, mcp `--bins` 425,
protocol 58, catalog/crate 2+2+2. **Los tres e2e de apply ejecutados por el
orquestador contra la imagen real (57,7 s): list, commit-por-writer-cambia-disco
e invalida el project_ref, y ACTION_STALE tras cambio de fuente — todos verdes.**
El "bloqueante del pin de bloat" del worker fue un **falso positivo**: el pin es
un hash de schema canónico, no de archivo; `git` no muestra cambio y
`test-release-smoke` pasa. `destructive=true` es correcto (coherente con las
tools de escritura M2).

## Informe literal del worker

# W08 report: `rust.analyzer.actions` and `rust.analyzer.action.apply` (M6-04/05)

## Task
Add the two remaining M6 tools on top of the uncommitted W07 work: tool 35, read-only; tool 36, which writes only through the single M2 writer. Close the V07 P3s, add the host grant, snapshots, wire and lifecycle tests, native end-to-end tests, and the docs. Nothing was committed and no Docker-backed test was run.

## Result
D1–D5 are implemented. Workspace `clippy -D warnings`, `fmt --check` and the architecture gate are clean. Every targeted test passes, and the docs link check reports no broken links in living documents.

One pre-existing problem blocks the release smoke independently of W08: the pinned hash for `rust.binary.bloat` no longer matches its committed snapshot. The prompt said to stop and report, so I left that pin alone (details under Open issues).

## Files changed
- **New**
  - `crates/mcp-server/src/stdio/analyzer/actions.rs`: the actions tool.
  - `crates/mcp-server/src/stdio/mutation/analyzer_action.rs`: the apply tool, its unit tests and the lifecycle tests.
  - Snapshots `analyzer-actions-tool.json` and `analyzer-action-apply-tool.json`.
- **Domain** (`analyzer.rs`): `RejectedAction { reason, title?, kind? }` and `validate_action_edits`.
- **Execution adapter**
  - `lsp_codec.rs`: new `action_labels`.
  - `analyzer_gateway.rs`: rejected candidates carry their title and kind; the digest doc is corrected.
  - `project_inspection.rs`: P3-6.
  - `analyzer_native.rs`: pattern update only.
- **Application** (`analyzer.rs`)
  - The listing now applies the preview's structural rules.
  - Rejected summaries carry title and kind.
  - The provenance encoder is public as `AnalyzerActionProvenance`.
- **mcp-server**
  - `stdio.rs`, `host_config.rs`, `main.rs`: registration and the new flag.
  - `analyzer.rs` and `schemas.rs`: shared range input and action wire types.
  - `mutation.rs` and `audit.rs`: the audit state is now generic over the output type, plus the per-kind validation-view guards and shared receipt helpers.
  - Tests: `analyzer/tests.rs`, `protocol.rs`, `analyzer_runtime.rs`, and the three `tools.len()` tests.
- **Scripts:** `release-smoke.py` and `test-release-smoke.py`.
- **Docs:** `tools.md`, `README.md`, `CHANGELOG.md`, `security-model.md`, `compatibility.md`, `client-configuration.md`, the M6 matrix, `implementation-status.md`, and a dated note in ADR-083.

## Tests executed
| Command | Result |
|---|---|
| domain `analyzer` | 45 passed |
| application `analyzer` | 30 passed |
| execution `--lib analyzer_gateway lsp_codec` | 84 passed |
| mcp `--bins` (the crate has no lib target) | 425 passed, 3 ignored; `analyzer_action` re-run after the clippy fix: 11 passed |
| mcp `--test protocol` | 58 passed |
| mcp `--test analyzer_runtime --no-run` | builds |
| `catalog_status`, `crate_inspect`, `crate_search` | 2 + 2 + 2 passed |
| `check-architecture.py` | PASS |
| `test-release-smoke.py` | 9 OK |
| `docs-hygiene.py links-check` | 0 broken in living documents |

## Evidence
- **New snapshot hashes**
  - `rust.analyzer.actions`: `a498d67d7189a0d44006327848e6db44896a585b134f45c28b86ad11879bb241`
  - `rust.analyzer.action.apply`: `e29532894766b31b39a50e7ff84256acb19b95a3c3c5bf83a197a096c93c784e`
- **The 34 existing snapshots are unchanged.** Git shows no modified snapshot file. The protocol bootstrap and a script compare all 34 served definitions to the files, with no mismatch. The five M2 tools keep their frozen `Output`/`ValidationView`.
- **How each V07 P3 was closed**
  - **Rejected actions without title/kind:** rejected entries now carry them from domain through the wire, with the title bounded to 256 scalars and control characters replaced. The listing also runs the preview's structural rules, so it never offers what preview would refuse.
  - **P3-2 (touched files):**
    - `files` carries a schema description warning that an action may rewrite up to 128 files.
    - The preview summary and the tool description both say it.
    - `guarantees_not_provided` includes `compile_verification`.
  - **P3-3 (cross-crate round trip):** `AnalyzerActionProvenance::encode` is decoded in mcp-server and compared field by field against exact JSON. Every field has a distinct value, so a reorder on either side fails.
  - **P3-4 (view per kind):** `m2_validation_view` refuses `AnalyzerActionApply`, and `analyzer_action_validation_for` accepts only that kind. Commit refuses a plan of another kind before the writer, and there are tests for both directions.
  - **P3-6:** `analyzer_source_fingerprint` now runs inside the quarantine scope in both W07 port methods.
  - **Digest doc:** it now says `image_id` and the gateway configuration are bound by the plan's provenance, not by the digest.
  - **P3-9:** three ignored native tests (list; preview→commit→receipt→disk bytes→reference invalidation; `ACTION_STALE`) run on a temp copy of the fixture.
- **Lifecycle without Docker:** these tests use a real macOS registry with a fake analyzer port and a fake writer.
  - A wrong digest is `CONFLICT`.
  - Commit reaches the writer exactly once, and a later receipt on the old `project_ref` is `PERMISSION_DENIED`; after reopening it is `committed`.
  - `ACTION_STALE` fires both at preview and at commit after a file edit, with the writer never called.
  - A plan of another kind is refused.

## Decisions
1. **Own output type** for the apply tool rather than reusing M2's, so no M2 schema can change. Everything that writes is reused unchanged: the shared plan store, diff and digest, commit/replay/receipt, the native store opened for this kind, preview retention, and the same audit event.
2. **Annotations:** `destructiveHint=true` like the M2 write tools, where ADR-083 §1 said false. Recorded in the ADR note.
3. **Grant handling:** without the grant every call is `unavailable/SANDBOX_DENIED` before any state is created. A grant for a different root or a stale reference stays `PERMISSION_DENIED`. Codes use SCREAMING_SNAKE_CASE like the other M6 tools.
4. **Commit staleness check:** commit first compares the live capture to the plan (read-only) and reports `ACTION_STALE` before asking the writer for anything. A receipt in state `aborted` is also `ACTION_STALE`.
5. **Filter vocabulary:** `only` accepts all 7 domain kinds, including `source_organize_imports`.
6. **Preview timeout:** preview takes an optional `timeout_seconds`.
7. **No runtime identity check at commit:** plans live only in memory and the analyzer identity is a compile-time constant, so a runtime rollback means a new process with no plans. Reasoning is in the ADR note.

## Risks
- The M2 audit code was refactored (a record struct and generic audit state). The M2 snapshots, protocol tests and audit tests still pass.
- Commit now does one extra capture (up to 16 MiB).
- The native tests assume actions exist at line 2, column 9 of the fixture, as the m6-11 cut observed.
- The listing checks per-file limits only; a bundle-level overflow is refused later, at preview.

## Open issues
- **Blocking, pre-existing:** `release-smoke.py` pins `rust.binary.bloat` to `3973462a…`, but the untouched committed snapshot hashes to `d3e51af9…`. The release smoke will refuse that tool regardless of W08. I did not change the pin.
- **`scripts/test-m6-runtime.py` does not discover `analyzer_runtime.rs`**, contrary to the prompt. It only selects the ignored tests of `analyzer_native.rs`, and the matrix records the `analyzer_runtime` wrapper as ungated. To run the new tests: `RUST_MCP_TEST_SOCKET=… cargo test -p rust-engineering-mcp --test analyzer_runtime -- --ignored --exact <name> --test-threads=1`.
- **`analyze` (M6-01) still hashes outside the quarantine scope.** I left it so an existing tool's behaviour doesn't change.
- **Failure outputs of the apply tool carry no analyzer identity data.**
- **`fixtures/analyzer-actions` is still not registered** in `fixtures/README.md` or `corpus-sha256.json` (carried over from W07).
