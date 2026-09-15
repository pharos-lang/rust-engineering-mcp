# W37 — `test-m8-clients-unit.py` hermetic against a missing `target/m1-17-inspector/`

## Root cause

`RunInspectorModeSeparationTests` (in `scripts/test-m8-clients-unit.py`) already
mocked `load_m3()` so `m3.run_bounded` never spawns Node, but `M8.run_inspector`
does two real filesystem operations *before* it ever calls `m3.run_bounded`:

- `bridge.open("xb")` — creates `ROOT/"target/m1-17-inspector"/m8-<attempt>-<mode>-bridge.mjs`
  (`scripts/test-m8-clients.py:1061-1064`, pre-fix), which fails if the
  `target/m1-17-inspector/` directory doesn't exist.
- `INSPECTOR.read_bytes()` — reads the real pinned Inspector CLI bundle
  (`target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/clients/cli/build/index.js`)
  to build the bridge file's contents.

On the SonarCloud Linux runner, `target/m1-17-inspector/` is never populated
(no Docker/Node bootstrap step in that job), so both operations raise
`FileNotFoundError`, and every test in `RunInspectorModeSeparationTests`
that reaches `M8.run_inspector` fails — `errors=8` in the reported failure.

## Fix

- **`scripts/test-m8-clients.py`**: extracted the bridge directory into a
  module-level constant `BRIDGE_DIR = ROOT / "target/m1-17-inspector"` and
  changed `run_inspector` to build `bridge` from `BRIDGE_DIR` instead of the
  inline string literal, so it is patchable from the test module.
- **`scripts/test-m8-clients-unit.py`**: `RunInspectorModeSeparationTests` now
  has a `setUp` that creates a throwaway temp directory per test, drops a
  small fixture file into it, and patches `M8.BRIDGE_DIR` → that temp
  directory, `M8.INSPECTOR` → the fixture file, and `M8.NODE` → a
  `/nonexistent/node` path (belt-and-suspenders — `m3.run_bounded` is always
  stubbed in this class already, so `NODE` is never actually spawned, but
  patching it too means no code path in these tests can resolve to the real
  Node binary or a real argv naming it). No behavior of `run_inspector`
  itself changed outside of the indirection through `BRIDGE_DIR`.

Only `scripts/test-m8-clients.py` and `scripts/test-m8-clients-unit.py` were
touched; `scripts/test-m8-performance-unit.py`, `scripts/test-m8-rollback-unit.py`,
and `scripts/test-contract-freeze.py` have no `BRIDGE_DIR`/`INSPECTOR`/Inspector-bundle
dependency (confirmed by grep) and needed no change.

## Hermeticity verification

Simulated the SonarCloud runner: `target/m1-17-inspector` renamed to
`target/m1-17-inspector.bak` for the duration of the run (restored in a
`finally`-equivalent regardless of outcome), `HOME` pointed at a fresh
directory created under `target/`, and `GIT_DIR=/nonexistent`. Ran every
suite in `.github/workflows/sonarcloud.yml`'s `Generate Python coverage`
step (the real list in the workflow file, which is broader than the one in
the task prompt) via plain `python3 <suite>.py` (`coverage` isn't installed
in this sandbox, but `coverage run` executes the same `__main__` path as a
plain interpreter invocation, so this is an equivalent check).

| Suite | With `target/m1-17-inspector/` (baseline) | Renamed away + fresh `HOME` + `GIT_DIR=/nonexistent` |
|---|---|---|
| `test-coverage-reports.py` | OK (3 tests) | OK (3 tests) |
| `test-gate-reporting.py` | OK (13 tests) | OK (13 tests) |
| `test-release-artifact.py` | OK (11 tests) | OK (11 tests) |
| `test-release-smoke.py` | OK (9 tests) | OK (9 tests) |
| `test-codex-model-qualifier.py` | OK (39 tests) | OK (39 tests) |
| `test-public-export.py` | OK (4 tests) | **FAILED (errors=1)** — pre-existing, out of scope |
| `test-summarize-m4-budgets.py` | OK (1 test) | OK (1 test) |
| `test-contract-freeze.py` | OK (20 tests) | OK (20 tests) |
| `test-docs-hygiene.py` | OK (10 tests) | **FAILED (errors=6)** — pre-existing, out of scope |
| `fixtures/rust-runtime/m6/test_provision.py` | OK (28 tests) | OK (28 tests) |
| `test-m6-provisioning.py` | OK (16 tests) | OK (16 tests) |
| `test-m6-runtime-unit.py` | OK (19 tests) | OK (19 tests) |
| `test-m6-clients-unit.py` | OK (109 tests) | **FAILED (errors=3)** — pre-existing, out of scope |
| `test-m8-performance-unit.py` (permitted file) | OK (79 tests) | **OK (79 tests)** |
| **`test-m8-clients-unit.py` (target of this task)** | OK (128 tests) | **OK (128 tests)** |
| `test-m8-rollback-unit.py` (permitted file, not in the workflow's Python-coverage step) | OK (40 tests) | OK (40 tests) |

The three "pre-existing, out of scope" failures are **not** caused by the
missing `target/m1-17-inspector/` directory and are **not** in this task's
permitted-files list (`scripts/test-m8-clients.py`,
`scripts/test-m8-clients-unit.py`, `scripts/test-m8-performance-unit.py`,
`scripts/test-m8-rollback-unit.py`, `scripts/test-contract-freeze.py`). They
are caused entirely by the `GIT_DIR=/nonexistent` stress condition, which
these tests are not designed to withstand because they legitimately shell
out to the *real* `git` binary as part of their own subject matter:

- `test-public-export.py` calls `public-export.py`'s `resolve_commit("HEAD")`,
  which runs `git rev-parse --verify HEAD^{commit}` against the repo — `git`
  reports `fatal: not a git repository: '/nonexistent'` once `GIT_DIR` is
  poisoned.
- `test-docs-hygiene.py`'s `Repo` test fixture runs `git init -q` to build a
  scratch repo per test — same poisoned-`GIT_DIR` failure.
- `test-m6-clients-unit.py` exercises the M6 client-qualification path,
  which also shells to `git` for repo state.

Confirmed by reproducing each with only `GIT_DIR=/nonexistent` set (no
directory rename, no `HOME` change): all three still fail identically, and
all three pass cleanly with `GIT_DIR` unset. In the real SonarCloud job,
`GIT_DIR` is never set to `/nonexistent` — it's a normal `actions/checkout`
clone — so these suites are unaffected there; the stress condition was only
useful to confirm that this task's target suites (`test-m8-clients-unit.py`,
`test-contract-freeze.py`, `test-m8-performance-unit.py`) don't have a
similar hidden `git` dependency. They don't: all three stayed `OK` under the
full combined stress (bridge dir renamed, fresh `HOME`, poisoned `GIT_DIR`).

`target/m1-17-inspector/` was restored immediately after the run regardless
of outcome (`mv target/m1-17-inspector.bak target/m1-17-inspector`), and the
scratch `HOME` directory was removed.

## Result

`scripts/test-m8-clients-unit.py` — the task's actual target — now passes
(`Ran 128 tests ... OK`) with `target/m1-17-inspector/` entirely absent,
which is what fails on the SonarCloud Linux runner today. No commit made
per instructions.
