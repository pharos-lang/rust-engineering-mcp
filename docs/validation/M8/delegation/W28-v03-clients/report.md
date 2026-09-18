# W28 — V03 corrections to the M8 client harness (C-1..C-7)

Worker: Claude Sonnet 5 (`claude -p --model sonnet --effort high`), harness role, no
subagents. Files touched: `scripts/test-m8-clients.py`, `scripts/test-m8-clients-unit.py`,
`scripts/m8-inspector-session.mjs`. No commit made.

## C-1 — `--run` did not exigir pinned versions; receipt wrote constants

- `run()` now calls `client_versions()` + `preconditions()` up front and aborts
  (`RuntimeError: mandatory preconditions unsatisfied: ...`) if any check outside a new
  `OPTIONAL_PRECONDITIONS` set (the Claude Code / Gemini CLI binary/version/auth checks,
  since those clients are `unavailable`-tolerant) is unsatisfied. Added `mandatory_unsatisfied()`.
- The receipt now carries `observed_versions` (the full `client_versions()` dict) at the
  top level, plus `head_commit`/`tree_dirty` (new `git_head_commit()`/`git_tree_dirty()`
  helpers, mirroring `scripts/contract-freeze.py`'s own pattern).
- `run_inspector()`, `codex_gate()`, `claude_gate()`, `gemini_gate()` each now take an
  `observed_version` parameter and write that into their own receipt's `"version"` field
  instead of the `INSPECTOR_VERSION`/`CODEX_VERSION`/`CLAUDE_VERSION`/`AGY_VERSION`
  constants. The constants remain, used only for `expected` pins in `client_versions()` /
  `preconditions()` / `preflight()`.

## C-2 — Codex oracle accepted any `tool`/`name` value containing `rust.project.open`

Added `codex_protocol_evidence(observation)`, which reads the same `protocol.jsonl` the
proxy already records for `client == "codex"` and requires: `rust.project.open` **and**
`rust.project.inspect` both observed as `tools/call` requests, and the unknown-tool refusal
observed either on the wire (the fake tool's `tools/call` immediately followed by a
server-direction row) or, failing that, in the model's own transcript events. `classification`
is `"passed"` only when all three hold (plus `exit_code == 0`). The returned dict now carries
a `protocol_evidence` block for auditability.

## C-3 — Generic negatives swallowed any exception as `protocol_error: true`

- `m8-inspector-session.mjs`'s `callGeneric()` now captures `error.code` from the thrown
  `McpError` (`rpc_code`), returning `null` only when no numeric code was present.
- `validate_generic_negative_rows()` now requires `rpc_code == -32601` for `unknown_tool`
  and `-32602` for `invalid_args`/`unknown_fields` (`GENERIC_RPC_CODES`).
- New `generic_negative_wire_confirmed(observation)`: the last `len(GENERIC_NEGATIVE_ROWS)`
  `tools/call` requests the Inspector session sent are, in order, exactly the four generic
  negatives (verified by position, since the plan is executed strictly sequentially); each
  must be immediately followed by one server-direction row in `protocol.jsonl`, proving the
  request reached the server rather than being short-circuited by the Inspector SDK's own
  client-side validation. `run_inspector()` calls this and aborts if any generic negative
  did not round-trip; the result is recorded as `generic_negatives_wire_confirmed` in the
  Inspector report.
- Live-validated against the committed server binary in an isolated Docker-free session
  (see Verification): observed codes were exactly `-32601` / `-32602` / `-32602` and all
  four rows confirmed on the wire.

## C-4 — `run()` returned 0 even when the receipt was `failed`

`run()` now returns `0 if receipt["status"] == "passed" else 1`.

## C-5 — Negative oracle accepted a null code and any declared code

- Every one of the 30 refusal rows in `NEGATIVE_ROWS` now carries a fixed `expected_code`,
  taken from the observed codes in `docs/validation/M8/clients/attempt-13/` (verified live
  against the committed binary — see Verification). The single `observation_only` row
  (`rust.catalog.status`) carries `expected_code: None`.
- `check_negative_row()` now requires a non-empty `expected_code` within the tool's own
  declared vocabulary for refusal rows, and rejects an `expected_code` on the
  `observation_only` row.
- `validate_negative_rows()` now rejects a `null` `error_code` on a refusal, and requires
  `code == expected["expected_code"]` (not merely membership in `declared_codes`).

## C-6 — Vocabulary extraction ignored `#[serde(rename)]`/`rename_all`

`declared_error_codes()` no longer guesses the case transform from
`tool in MUTATION_TOOLS`. New `enum_case_transform(source, enum_pos, enum_name)` reads the
enum's own contiguous `#[serde(...)]` attribute block immediately above the `enum X {`
declaration for a `rename_all = "..."` value, mapping `SCREAMING_SNAKE_CASE`/`snake_case`
via `RENAME_ALL_TRANSFORMS`; an enum with no `rename_all` or an unsupported convention is a
hard `RuntimeError`, not a silent guess. New `enum_variants(block)` honors an explicit
per-variant `#[serde(rename = "...")]` (none currently exist in the 29 `Code`/`Reason`
enums this harness reads, but the parser now handles them instead of silently mis-deriving
the code). Verified this does not change any of today's 29 enums' derived vocabularies
(cross-checked `rust.project.open`/`rust.check`/`rust.manifest.patch` against the committed
source — see Verification).

## C-7 — Hygiene

- Receipt now carries `head_commit`/`tree_dirty` (see C-1).
- `proxy_parser.add_argument("--observation", ...)` no longer uses `type=pathlib.Path`;
  the string is wrapped in `pathlib.Path(...)` at the call site instead.
- `/private/tmp` literals removed: `BOGUS_OPEN_PATH` and the Docker-free fallback socket
  path both live under `ROOT / "target"` now.
- All `tempfile.mkdtemp(...)` / `tempfile.TemporaryDirectory(...)` calls in this script
  (`codex_gate`'s Codex `auth.json` copy, `claude_gate`'s private HOME, `eof_gate`'s scratch
  dir) now pass `dir=str(ROOT / "target")` instead of the system temp directory.
- Module docstring and the `NEGATIVE_ROWS` comment corrected: "one structured Docker-free
  refusal per `stable` tool" → "one structured Docker-free negative row per `stable` tool
  (30 refusals plus the single `rust.catalog.status` passed-observation row)".

## Verification

- `python3 -B scripts/test-m8-clients-unit.py` → **92 tests, OK** (36 new/changed tests
  covering all seven corrections: precondition gating, git receipt metadata, Codex protocol
  evidence, generic-negative wire confirmation, the fixed-expected-code oracle, and the
  `rename_all`-derived vocabulary including an explicit per-variant rename and an
  unrelated-enum-in-the-same-file disambiguation case).
- `python3 -B scripts/test-m8-clients.py --preflight` → `"status": "ready"`,
  `"unsatisfied": []`.
- `python3 -B scripts/test-gate-reporting.py` → **13 tests, OK**.
- Additionally, per the delegation brief's allowance to validate the `.mjs` with an isolated
  Docker-free Inspector session: called `run_inspector()` directly (not `--run`) against the
  committed `target/release/rust-engineering-mcp`, in a scratch `target/m8-isolated-*` dir,
  cleaned up after. Results: `contract_equality: True`; all 30 negative rows landed exactly
  on their new fixed `expected_code` (matching `attempt-13`'s observations byte for byte);
  all 4 generic negatives reported the correct JSON-RPC code (`-32601` for `unknown_tool`,
  `-32602` for `invalid_args`/`unknown_fields`) and were confirmed on the wire; the
  Inspector report's `"version"` field carried the observed `2.5.0`, not a hardcoded literal.
  `node --check scripts/m8-inspector-session.mjs` passed.

No `--run` (full matrix) was executed, per instructions. No commit made.
