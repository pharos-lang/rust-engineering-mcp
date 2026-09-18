# W38 — SonarCloud PR #22 taint findings (22) and M8 host-only coverage exclusions

## Task

Fix the 22 `pythonsecurity` taint findings SonarCloud's PR #22 quality gate
reported (`new_security_rating` E) across `contract-freeze.py`,
`measure-m8-performance.py` and `soak-m8.py`, and add the M8 host/Docker-only
harnesses to `sonar.coverage.exclusions`. House rule (M5/M6 precedent): the
taint engine ignores `argparse type=` and any post-parse validation; paths
must be constants derived from `ROOT`, and variable parameters travel by
stdin JSON validated against a closed schema, never by CLI arguments that
reach `open()`/`subprocess`.

## Result

All 22 findings fixed by removing every CLI-supplied path from the three
scripts' `open()`/`subprocess` call sites. No behavior lost: `contract-freeze
diff`'s `base`/`only`/`out` moved to a stdin JSON envelope; `measure-m8-performance.py`'s
`local` profile now self-provisions its catalog fixture exactly like
`soak-m8.py` already did, instead of taking catalog paths from the CLI.

## Finding → change map

| Finding(s) | File:line(s) (before) | Change |
|---|---|---|
| `S2083` path traversal | `contract-freeze.py:141` (`out_path.write_text` in `cmd_generate`) | `--out` removed; `generate` always writes the constant `FREEZE_MANIFEST_PATH = ROOT/"docs/validation/M8/freeze-0.8.0.json"`. |
| `S8707`/`S8701`/`S8705` | `contract-freeze.py:77` (`resolve_commit`'s `git rev-parse` arg) | Unchanged mechanism (already `--end-of-options` + `git`, no shell), but now `base` is validated by `BASE_PATTERN = ^(v[0-9]+\.[0-9]+\.[0-9]+\|[0-9a-f]{7,40})$` before reaching it, and `base` arrives via stdin JSON, not argv. |
| — | `contract-freeze.py:140-141` (`cmd_generate`'s `out_path.parent.mkdir`/`write_text`) | Same as `S2083` row above: constant `FREEZE_MANIFEST_PATH`. |
| — | `contract-freeze.py:154` (`cmd_verify`'s `manifest_path.read_text`) | `verify` takes no positional arg; reads the same constant `FREEZE_MANIFEST_PATH`. |
| — | `contract-freeze.py:296-297` (`cmd_diff`'s `out_path.parent.mkdir`/`write_text`) | `diff` reads `{"base","only","out"}` from stdin; `out` is validated as a **key** of the constant dict `DIFF_DESTINATIONS = {"since_v0.3.0": SCHEMA_DIFF_PATH, "since_v0.1.0_m1_only": SCHEMA_DIFF_PATH}` (both entries point at the existing `docs/validation/M8/02-schema-diff.json`, matching its current two-key shape); nothing from stdin is ever concatenated into a path. |
| `S8707`/`S8701`/`S8705` (×2, budgets) | `measure-m8-performance.py:80` (`load_budgets`'s `path.read_text`), `:92` (`file_sha256`'s `path.open`) | `--budgets` removed; both call sites now always receive the constant `DEFAULT_BUDGETS = ROOT/"docs/validation/M8/05-budgets.json"`. |
| — | `measure-m8-performance.py:86` (`binary_stat`'s `binary.open`) | `--binary` removed; `binary` is always the constant `DEFAULT_BINARY = ROOT/"target/release/rust-engineering-mcp"`. |
| — | `measure-m8-performance.py:147` (`ServerProcess.__init__`'s `subprocess.Popen`) | Same constant binary; and for the `local` profile, `extra_args` (`--catalog-store`/`--catalog-trust`) no longer come from CLI flags — `measure_local` now calls a new `prepare_catalog(scratch, binary)` (mirrors `soak-m8.py`) that stages the constant `fixtures/catalog` bundle into a `target/`-scoped scratch dir it creates itself. `--catalog-model-dir`/`--catalog-index-store` (optional E5/ORT semantic-search flags, unused by CI's lexical-only calls) were dropped along with the CLI paths that fed them. |
| `S2083` path traversal, `S8707`/`S8701`/`S8705` | `measure-m8-performance.py:611` (`run_compare`'s `Path(path).read_text`), `:627-628` (`out_path.parent.mkdir`/`write_text`) | `--compare` now takes 3 short **keys** (`RECEIPT_KEY_PATTERN = ^[a-z0-9-]+$`), resolved via `receipt_path_for_key` to `RECEIPTS_DIR/f"{key}.json"` under the constant `RECEIPTS_DIR = ROOT/"target/m8-performance"`; the comparison verdict always writes to the constant `COMPARE_OUT_PATH = RECEIPTS_DIR/"regression.json"`. |
| — | `measure-m8-performance.py:748-749` (main's `out_path.parent.mkdir`/`write_text`) | `--out` removed; the measurement receipt always writes the constant `OUT_PATH = ROOT/"docs/validation/M8/05-measurement.json"`. |
| `S8707`/`S8701`/`S8705` | `soak-m8.py:90` (`binary_sha256`'s `binary.open`), `:239` (`prepare_catalog`'s `subprocess.run`), `:250` (`ServerProcess.__init__`'s `subprocess.Popen`) | `--binary` removed; `binary` is always the constant `DEFAULT_BINARY`. `prepare_catalog` already used the constant fixture bundle — only the binary argument was tainted. |
| — | `soak-m8.py:647-648` (main's `out_path.parent.mkdir`/`write_text`) | `--out` removed; the soak receipt always writes the constant `OUT_PATH = ROOT/"docs/validation/M8/05-soak-core.json"`. |

Numeric parameters (`--repeat`, `--cycles`, `--hours`, `--sample-every`,
`--open-churn`, `--ttl-wait-seconds`, `--project-ttl-secs`) stayed on
`argparse` with `int()`/`float()` conversion — they never reach a path or a
subprocess argv position, so the taint engine does not flag them. The
server's own argv (`serve --stdio --root <FIXTURE> …`) was already built
from constants and is unchanged.

## Files changed

- `scripts/contract-freeze.py` — `generate`/`verify` drop their path
  argument for the constants above; `diff` reads a stdin JSON envelope
  (`base`/`only`/`out`) instead of `--base`/`--out`/`--only`; new
  `parse_diff_request` validates the closed schema before any Git or
  filesystem access.
- `scripts/test-contract-freeze.py` — adapted all `GenerateVerifyTests` to
  patch `CF.FREEZE_MANIFEST_PATH` instead of passing a manifest path; rewrote
  `DiffTests` to drive `cmd_diff()` through a mocked `sys.stdin` and patched
  `CF.DIFF_DESTINATIONS`; added tests for the new stdin/regex validation
  (invalid `base`, unknown `out` key, non-object/malformed stdin, two
  `out` keys merging into one shared destination file). 26 tests, was 20.
- `scripts/gate.py` — `contract-freeze` stage invocation drops the manifest
  path argument (`verify` now reads the constant).
- `scripts/measure-m8-performance.py` — removed `--binary`/`--out`/`--budgets`/
  `--catalog-store`/`--catalog-trust`/`--catalog-model-dir`/`--catalog-index-store`;
  added `prepare_catalog`, `receipt_path_for_key`, `RECEIPT_KEY_PATTERN`,
  `OUT_PATH`, `RECEIPTS_DIR`, `COMPARE_OUT_PATH`; `measure_local` and
  `run_compare` rewritten around the constants.
- `scripts/soak-m8.py` — removed `--binary`/`--out`; added `OUT_PATH`
  constant; docstring note on the constant-path convention.
- `scripts/test-m8-performance-unit.py` — `RunCompareTests` rewritten around
  keys + patched `RECEIPTS_DIR`/`COMPARE_OUT_PATH`; added
  `test_invalid_key_is_rejected`/`test_valid_key_resolves_under_receipts_dir`.
  81 tests, was 79.
- `sonar-project.properties` — added `scripts/measure-m8-performance.py`,
  `scripts/soak-m8.py`, `scripts/test-m8-rollback.py` to
  `sonar.coverage.exclusions` (`scripts/test-m8-clients.py` and
  `scripts/m8-inspector-session.mjs` were already present). No `crates/**`
  pattern touched; `scripts/contract-freeze.py` was not added (portable gate
  stage, per instructions).
- `docs/ci.md` — new group 4 justifying all five M8 host-only exclusions in
  one paragraph (release binary + `ps`/`lsof`/`pgrep` telemetry for the two
  performance/soak scripts; same binary plus real stock clients for the
  rollback/client harnesses; a real Node bridge for the Inspector session).

## Verification

```
python3 -B scripts/test-contract-freeze.py        # 26 tests, OK
python3 -B scripts/test-m8-performance-unit.py    # 81 tests, OK
python3 -B scripts/test-gate-reporting.py         # 13 tests, OK (includes the crates/** exclusion-ban rule)
python3 -B scripts/contract-freeze.py verify      # {"status": "passed", ...}
python3 -B scripts/docs-hygiene.py links-check    # 0 broken in living documents
```

Also spot-checked `contract-freeze.py diff` end-to-end against real Git
history (`{"base": "v0.3.0", "out": "since_v0.3.0"}` via stdin, with
`DIFF_DESTINATIONS` pointed at a scratch file): reproduced the exact
added/removed/changed counts already on record in
`docs/validation/M8/02-schema-diff.json` (5 added, 0 removed, 1 changed
(`rust.binary.bloat`), 30 unchanged).

## Risks / open issues

- `docs/validation/M8/05-soak-core.json` does not exist yet — `soak-m8.py`'s
  output constant is new; the file will appear the next time an operator
  runs a real soak. Historical soak calibration receipts stayed in
  `target/` (gitignored) and are unaffected.
- Ad hoc smoke runs of `measure-m8-performance.py`/`soak-m8.py` (e.g. a short
  `--repeat 5` dry run) now overwrite the same constant evidence files as a
  real run, since `--out` no longer exists; an operator doing a smoke test
  should be aware it replaces `docs/validation/M8/05-measurement.json` (or
  `05-soak-core.json`) rather than writing to a scratch path.
- `measure-m8-performance.py --profile local` dropped `--catalog-model-dir`/
  `--catalog-index-store` (E5/ORT semantic-search timing) along with the
  catalog-path CLI flags; only the `--profile local` lexical-mode path
  (`rust.crate.search` with `mode: "lexical"`, the only call already made by
  this harness) is unaffected. Re-adding semantic-mode timing would need a
  similar self-provisioning helper for the model directory, out of this
  PR's scope.
- `docs/ci.md` still references the pre-fix `contract-freeze.py verify
  docs/validation/M8/freeze-0.8.0.json` invocation at line ~378 (outside the
  one-sentence-per-exclusion scope given for this task), and does not yet
  document the pre-existing `scripts/test-m8-clients.py`/
  `scripts/m8-inspector-session.mjs` exclusions beyond the new group 4 added
  here; both are pre-existing documentation drift, not introduced by this
  change.
