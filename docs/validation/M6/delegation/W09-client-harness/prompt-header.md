# W09 — M6 stock-client qualification harness (`scripts/test-m6-clients.py`)

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Role: implementation worker (a Python harness + its unit test + wiring; no product code). Orchestrator: Claude Fable 5.1 (running as Opus 4.8). You may not spawn subagents. **Never run a command in the background.** You may run the harness's `--write-preflight`/unit tests (no Docker), but NOT the `--run`/`--with-runtime` modes (the orchestrator runs those with Docker). Do not commit.

## Goal (G4/G8 for M6)

A source-bound stock-client qualification harness for the five M6 analyzer tools (`rust.analyzer.symbols`, `.references`, `.diagnostics`, `.actions`, `.action.apply`), mirroring `scripts/test-m5-clients.py` exactly in structure, conventions, receipt shape, SonarCloud-taint discipline (constant paths, no argv-derived opens, no `/tmp` literals, worker params via stdin), and the two stock clients: the MCP Inspector (deterministic conversion of every planned call) and Claude Code (`claude -p`, version 2.1.267, model `claude-sonnet-5`, restricted to the configured server) as the model-directed agentic client. Read `scripts/test-m5-clients.py`, `scripts/test-m5-clients-unit.py`, `scripts/m5-inspector-session.mjs`, `scripts/test-m3-clients.py` (the reused `load_m3()` proxy/preflight helpers), and `docs/validation/M5/clients.json` (the receipt shape) before writing anything. Reuse `load_m3()` for the proxy, source hashing, `save_json`, and the client drivers wherever M5 does.

## Modes (closed by default, exactly like M5)

- **default (preflight)**: client-free, Docker-free. Re-derive the advertised inventory from the server sources (36 tools; the five analyzer tools present and pushed after the M5 tools), check every planned call is answered exactly as the tool sources say, report host preconditions (Inspector 2.5.0, Claude Code 2.1.267, the M6 image `sha256:f39a5b33…`, the Docker socket, the `--allow-analyzer-action-write` grant), write nothing unless `--write-preflight`.
- **`--run`**: the Docker-free matrix — every analyzer call is a declared refusal the server produces before a container can exist (no `--rust`/no grant → `unavailable/SANDBOX_DENIED`), proven by a socket path inside the harness's private dir that is never created and is asserted absent afterward.
- **`--run --with-runtime`**: the real matrix through the admitted M6 image. It needs the real Docker socket, the M6 image, and (for the write row) the `--allow-analyzer-action-write` grant on a **temporary copy** of a fixture (never a repo fixture — the write mutates it).

## The plan rows (both clients convert/drive these)

On `fixtures/valid-basic` and `fixtures/analyzer-references` (read-only) and a **temp copy** of `fixtures/analyzer-actions` (for the write):
1. `rust.analyzer.symbols {scope:document, file:"src/lib.rs"}` → positive: `passed`, `readiness quiescent`, `health ok`, `complete`, symbols present.
2. `rust.analyzer.symbols {scope:workspace, query:"add"}` → positive.
3. `rust.analyzer.references {file:"src/lib.rs", position, include_declaration:true}` on `fixtures/analyzer-references` → positive: exactly one declaration.
4. `rust.analyzer.diagnostics {file:"src/lib.rs"}` on `fixtures/valid-basic` → positive: `complete`, syntax-only (the honest M6-03 contract; empty is a valid answer).
5. `rust.analyzer.actions {file, range}` on the temp `analyzer-actions` → positive: ≥1 applicable action with a digest.
6. `rust.analyzer.action.apply` preview→commit→receipt on that digest → the write lands (the file changes on disk), the receipt is `committed`, the pre-commit `project_ref` is invalidated. A **negative** row: a stale digest → `blocked/ACTION_STALE`.
7. A failure row for the reads: `rust.analyzer.symbols` with a bad `file` (non-`.rs` or absent) → the declared `blocked/FILE_NOT_IN_SNAPSHOT` or `invalid_params`.

- **Inspector**: converts every planned row deterministically (extend `m6-inspector-session.mjs`, mirroring `m5-inspector-session.mjs`), reading each response; for the write row it performs preview then commit then receipt with the ids from the session.
- **Claude Code (model-directed)**: opens the project, calls the read tools, and performs the apply preview→commit itself (discovers the action, previews it, reviews the diff, commits with an idempotency key, reads the receipt), verifying the source changed. The oracle requires each planned call exactly once with the session-issued ids, the `claude-sonnet-5` model on every assistant message (the fallback-model check M5 uses), `permission_denials: []`, no credential-shaped bytes staged, and the model's own project_ref/plan ids (not Inspector's). The cancel case (G4): one analyzer call cancelled mid-flight is reported `cancelled` with joined cleanup.

## Deliverables

- `scripts/test-m6-clients.py` — the harness (preflight + `--run` + `--run --with-runtime` + the `proxy` subcommand reused from M5), writing the receipt to `docs/validation/M6/clients.json` and per-attempt state under `docs/validation/M6/clients/` (the raw client outputs/state are `.gitignore`d exactly as M5's are; the receipt + protocol.jsonl are the kept bytes).
- `scripts/test-m6-clients-unit.py` — the unit tests of the harness's pure logic (plan derivation, oracle strictness, the model-message model check, receipt assembly), no Docker/network, ≥80% of the new lines.
- `scripts/m6-inspector-session.mjs` — the Inspector driver (mirror the M5 one).
- Wiring: add `scripts/test-m6-clients.py` and `scripts/test-m6-clients-unit.py` to the SonarCloud coverage list in `.github/workflows/sonarcloud.yml` (after the m6-provisioning ones); if the harness is host/Docker-only (like `test-m5-clients.py`), instead add it to `sonar.coverage.exclusions` in `sonar-project.properties` exactly as `test-m5-clients.py` is treated — match M5's precedent (check which list M5's harness is in and follow it). Add `test-m6-clients-unit.py` to the coverage-run list (it is unit-testable).
- Run `python3 -B scripts/test-m6-clients-unit.py` and `python3 -B scripts/test-m6-clients.py --write-preflight` (no Docker) and report their output. Do NOT run `--run`/`--with-runtime`.

## Constraints

Python stdlib + the reused M3/M5 helpers only. SonarCloud taint: constant output paths (the M5 `beside_default`/constant-path pattern), no argv-derived path opened, no `/tmp` literals (use the harness private dir under the repo `target/` or `tempfile` as M5 does — match M5), worker params via stdin, `os.killpg`/`start_new_session` for child cleanup. The write row must operate on a **temp copy**, never a repo fixture. Nothing credential-shaped staged under the attempt dir. Report: Task / Result / Files changed / Tests executed (preflight + unit output) / Evidence / Risks / Decisions / Open issues.
