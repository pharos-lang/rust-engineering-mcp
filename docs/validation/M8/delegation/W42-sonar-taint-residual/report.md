# W42 — SonarCloud PR #22: the 6 residual taint findings after W38

## Task

Fix the 6 remaining `pythonsecurity` taint findings (`new_security_rating` E:
2 `BLOCKER` + 4 `MAJOR`) left after W38 across `contract-freeze.py` and
`measure-m8-performance.py`/`soak-m8.py`. Unlike W38 (CLI-supplied paths
reaching `open()`/`subprocess`), every sink here already writes to a
constant path under `ROOT`; the engine was following either a genuinely
tainted path built from a stdin-keyed subscript, or tainted *content*
(argv strings, file bytes) landing in the JSON that gets serialized and
written.

## Finding → change map

| # | Rule | Sink | Change | Why the engine stops following the flow |
|---|---|---|---|---|
| 1 | `S2083` BLOCKER | `contract-freeze.py:349` (now `394`) `out_path.write_text(...)` | `DIFF_DESTINATIONS` (a dict mapping stdin-controlled keys to `SCHEMA_DIFF_PATH`) removed; replaced with `DIFF_OUT_KEYS`, a `frozenset` of the two valid *labels*. `parse_diff_request` now returns `(base, out_key, only)` — no path. `cmd_diff` writes to the literal constant `SCHEMA_DIFF_PATH` unconditionally; `out_key` is used only as a key *inside* the JSON object, never to select a `pathlib.Path`. | The write's destination is no longer `SOME_DICT[out_key]` (a subscript keyed by validated-but-still-tainted stdin content) — it is the module-level constant `SCHEMA_DIFF_PATH` referenced directly, so there is no tainted-key-to-path edge left for the engine to follow. |
| 2 | `S2083` BLOCKER | `contract-freeze.py:157` (now `200`) `FREEZE_MANIFEST_PATH.write_text(...)` | Added `known_tool_name()` (regex `TOOL_NAME_PATTERN = ^rust\.[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)*$`, returns `match.group(0)`) and `known_annotations()` (rebuilds the dict key-by-key from the closed `ANNOTATION_KEYS` tuple, `isinstance(value, bool)` checked, returns `bool(value)`). `load_current_tools` now derives `name` via `known_tool_name(spec["name"], path)`; `tool_entry` derives `annotations` via `known_annotations(spec.get("annotations", {}), name)`. A snapshot with an out-of-grammar name or a non-boolean/unknown annotation key now raises `SystemExit` instead of flowing through. | `spec` comes from `path.read_bytes()` (a file-content taint source), and `spec["name"]`/`spec["annotations"]` used to flow verbatim into the manifest dict that `cmd_generate` writes. `match.group(0)` and the rebuilt annotations dict are values re-derived from a closed grammar/keyset, not slices of the tainted bytes — the same "regex match re-derivation" and "membership selection" sanitizers used elsewhere in the house rule, just applied to file content instead of argv. |
| 3 | `S2083` + `S8707` | `measure-m8-performance.py:658` (now `669`) `COMPARE_OUT_PATH.write_text(...)` | `receipt_path_for_key` now returns `(path, validated_key)`, where `validated_key = match.group(0)` from the existing `RECEIPT_KEY_PATTERN.match(key)`. `run_compare` stores `payload["receipts"] = validated_keys` (the re-derived keys) instead of the raw `receipt_keys` argv list. | `args.compare`'s raw strings used to be copied straight into `payload["receipts"]`. They are now replaced by `match.group(0)` of the regex that already validated them — a fresh string object re-derived from a closed pattern, not the original argv value. |
| 4 | `S8707` MAJOR | `measure-m8-performance.py:752` (now `768`) `OUT_PATH.write_text(...)` | Added `PROFILE_CHOICES = ("core", "local")`; `argparse`'s `choices=` now points at that tuple. In `main()`, `profile = next(choice for choice in PROFILE_CHOICES if choice == args.profile)` re-derives the value by membership instead of using `args.profile` directly; `operator_attested = bool(args.operator_attested)` re-derives the flag through the `bool()` builtin. Both `profile` and `operator_attested` (not `args.*`) are used everywhere downstream, including the two receipt fields. | `args.profile`/`args.operator_attested` used to land in the receipt untouched; `argparse`'s `choices=` validation doesn't register as a sanitizer for the engine. Selecting the equal element out of a hardcoded literal tuple, and wrapping the flag in `bool()`, are the same class of "re-derive from a closed domain" operations already recognized for numeric argv (`int()`/`float()`), generalized to strings-from-`choices` and to booleans. |
| 5 | `S8707` MAJOR | `soak-m8.py:647` (now `653`) `OUT_PATH.write_text(...)` | Same treatment as #4: added `PROFILE_CHOICES = ("core", "local")` used by `choices=`; `profile = next(choice for choice in PROFILE_CHOICES if choice == args.profile)` computed once in `main()` and used both for the `--profile local` guard and the final `print` (the receipt's own `"profile"` field was already the literal `"core"`, unchanged); `operator_attested = bool(args.operator_attested)` computed before being passed into `run_core_soak(...)`, replacing the raw `args.operator_attested` argument. | `run_core_soak`'s `operator_attested` parameter used to receive `args.operator_attested` (a `store_true` flag with no re-deriving call in between) straight into the receipt. `bool(...)` is the same builtin-coercion sanitizer pattern as `int()`/`float()`, applied to the one flag that had no such call. |

## Design notes / why not a literal tool-name allowlist

The obvious reading of "conjunto cerrado de nombres conocidos" for finding
#2 would be a hardcoded `frozenset` of the 36 real tool names. That was
rejected: `test-contract-freeze.py`'s `GenerateVerifyTests` and `DiffTests`
deliberately exercise `load_current_tools()`/`cmd_diff()` against synthetic
fixture names (`rust.example.stable`, `rust.example.keep`, `rust.example.add`,
etc.) to stay hermetic and decoupled from the real snapshot set — a literal
36-name allowlist would make those tests fail loudly instead of the actual
bug they're designed to catch. A structural regex grammar (`TOOL_NAME_PATTERN`)
is the same "closed set" defense already used elsewhere in this file
(`BASE_PATTERN`, `RECEIPT_KEY_PATTERN` in `measure-m8-performance.py`) and
is satisfied by both the 36 real names and the tests' synthetic ones, so no
test needed to change for that part. `RealRepositoryClassificationTests`
(asserting `load_current_tools()` finds exactly the 36 real names) still
passes unmodified, since all 36 real names match the grammar.

## Tests adapted

`DIFF_DESTINATIONS` (a `dict[str, Path]`) no longer exists — its two
production keys pointed at the same `SCHEMA_DIFF_PATH`, which is exactly
the degenerate shape the finding called out. `test-contract-freeze.py`'s
`DiffTests` patched that dict directly, so it was adapted, not left broken:

- `setUp` now patches `CF.SCHEMA_DIFF_PATH` (to the test's scratch file) and
  `CF.DIFF_OUT_KEYS` (to `frozenset({self.OUT_KEY})`) instead of
  `CF.DIFF_DESTINATIONS`.
- `test_a_second_out_key_merges_into_the_shared_destination` now patches
  `CF.DIFF_OUT_KEYS` to `frozenset({self.OUT_KEY, other_key})`; both keys
  already share the single patched `SCHEMA_DIFF_PATH`, which is the point
  being tested.

`receipt_path_for_key` changed its return type from `Path` to
`tuple[Path, str]`. `test-m8-performance-unit.py`'s
`test_valid_key_resolves_under_receipts_dir` was adapted to unpack both and
assert the re-derived key equals the input for a valid key; no test asserted
on the old single-`Path` return elsewhere, and `RunCompareTests` (which
drives `run_compare` end-to-end) needed no change since it only inspects
`payload["verdicts"]`/`payload["regressed"]`, not `payload["receipts"]`.

No test referenced `args.profile`/`args.operator_attested`/`PROFILE_CHOICES`
directly (those are only exercised through `main()`, which none of the unit
tests call), so no further test changes were needed for findings #4/#5.

## Restricción inamovible: manifest byte-identity

```
cp docs/validation/M8/freeze-0.8.0.json <scratch>/freeze-0.8.0.json.bak
python3 -B scripts/contract-freeze.py generate
diff -u <scratch>/freeze-0.8.0.json.bak docs/validation/M8/freeze-0.8.0.json
```

Result: the only lines that differ are `generated_utc` (a timestamp,
non-deterministic by design on every `generate` run, before and after this
change) and `head_commit` (advanced because commits landed on this branch
since the manifest was last generated, unrelated to this change). Every
other byte — `canonical`, `format_version`, `preview_count`, `stable_count`,
`tool_count`, and all 36 `tools` entries (`stability`, `annotations`,
`input_schema_sha256`, `output_schema_sha256`, `description_sha256`,
`snapshot_sha256`) — is identical. The file was restored from the backup
afterward; `git status --porcelain -- docs/validation/M8/freeze-0.8.0.json`
confirmed zero diff against the committed oracle.

## Verificación obligatoria (cifras)

```
$ python3 -B scripts/test-contract-freeze.py
Ran 26 tests in 0.025s
OK

$ python3 -B scripts/test-m8-performance-unit.py
Ran 81 tests in 0.005s
OK

$ python3 -B scripts/test-m8-rollback-unit.py
Ran 42 tests in 0.016s
OK

$ python3 -B scripts/contract-freeze.py verify --strict
{"class_changed": [], "format_errors": [], "preview_changed": [], "stable_changed": [], "status": "passed"}

$ python3 -B scripts/docs-hygiene.py links-check
links-check: 2979 links resolved; 0 broken in living documents; 5 point at
evidence excluded by .gitignore; 459 broken in frozen records
```

`0 broken in living documents` matches the W38 baseline; the 459 "broken in
frozen records" and 5 gitignore-excluded links are pre-existing and out of
this task's scope (frozen historical records referencing evidence files by
design, unrelated to these three scripts).

## Files changed

- `scripts/contract-freeze.py` — `DIFF_DESTINATIONS` replaced by
  `DIFF_OUT_KEYS`; new `TOOL_NAME_PATTERN`/`ANNOTATION_KEYS` constants and
  `known_tool_name`/`known_annotations` helpers; `tool_entry`/
  `load_current_tools`/`parse_diff_request`/`cmd_diff` updated to use them;
  docstring updated.
- `scripts/measure-m8-performance.py` — new `PROFILE_CHOICES` constant;
  `receipt_path_for_key` returns `(path, validated_key)`; `run_compare`
  stores validated keys; `main()` re-derives `profile`/`operator_attested`
  before using them anywhere, including the receipt.
- `scripts/soak-m8.py` — new `PROFILE_CHOICES` constant; `main()` re-derives
  `profile`/`operator_attested` the same way.
- `scripts/test-contract-freeze.py` — `DiffTests` adapted to patch
  `SCHEMA_DIFF_PATH`/`DIFF_OUT_KEYS` instead of the removed
  `DIFF_DESTINATIONS`.
- `scripts/test-m8-performance-unit.py` —
  `test_valid_key_resolves_under_receipts_dir` adapted to the new
  `receipt_path_for_key` return shape.
- `scripts/test-m8-rollback-unit.py` — not touched; no dependency on any of
  the above.

## Scope discipline

`sonar-project.properties` was not touched. No file was added to
`sonar.coverage.exclusions` or otherwise excluded from analysis; every fix
is a code change to the flagged flow itself. No file outside the six
allowed paths was modified. No commit was made.
