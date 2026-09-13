# W09e — pacing entre lotes de cliente (Docker settle + cooldown antes del lote de Claude)

| Modelo | Claude Sonnet 5 (High) | Inicio/fin UTC 2026-09-13T06:15:10Z / 2026-09-13T06:21:55Z | 105 unit OK, `--run` verde |
| --- | --- | --- | --- |

---

Both green. No commit made, as instructed.

## Report

**Task**: Insert a bounded Docker settle between the Inspector runtime batch and the Claude Code runtime batch in `--with-runtime` mode, to give the host fresh capacity after the Inspector's ~10 rust-analyzer container spawns.

**Result**: Implemented and verified in foreground; `--with-runtime` itself was not run (per instructions — that's the orchestrator's job with real Docker).

**Files changed**:
- `scripts/test-m6-clients.py`
- `scripts/test-m6-clients-unit.py`

**Where and how Docker is settled**:
- New constants: `CONTAINER_LABEL_FILTER = "--filter=label=org.rust-mcp.execution=true"` (same label the gateway's own `analyzer_native.rs::residue` uses), `CONTAINER_SETTLE_TIMEOUT_SECONDS = 60`, `CONTAINER_SETTLE_POLL_SECONDS = 2`, `RUNTIME_BATCH_COOLDOWN_SECONDS = 20`.
- `running_analyzer_containers(socket)`: runs `docker -H unix://<socket> container ls <label filter> --format={{.Names}}`, returns `[]` on any Docker error rather than raising.
- `settle_docker_between_batches(socket)`: polls `running_analyzer_containers` every `CONTAINER_SETTLE_POLL_SECONDS` until empty or `CONTAINER_SETTLE_TIMEOUT_SECONDS` elapses, then sleeps `RUNTIME_BATCH_COOLDOWN_SECONDS`; returns `{"waited_seconds", "cooldown_seconds"}`.
- Wired into `run()` right between `receipt["inspector"][RUNTIME] = inspector_gate(...)` and `receipt["claude_code"][RUNTIME] = claude_gate(...)`, using the same `socket_path` the runtime argv already derives. Result stored as `receipt["docker_settle"]`, initialized to `None` and left `None` on the Docker-free path. Docker-free mode and inter-row pacing within a batch are untouched.

**Tests**:
- Added `DockerSettleTests` in `scripts/test-m6-clients-unit.py`: unit-level coverage of `running_analyzer_containers` (names parsing, failure→empty) and `settle_docker_between_batches` (poll-then-cooldown, and give-up-at-bound-but-still-cooldown), plus two `run()`-orchestration tests — one confirming `settle_docker_between_batches` is never called on the docker-free path (`receipt["docker_settle"] is None`), one confirming it's called exactly once, in order, between the Inspector-runtime and Claude-runtime gates, on the runtime path (mocked `preflight`/`inspector_gate`/`claude_gate`, no real Docker/Node/Claude).
- `python3 -B scripts/test-m6-clients-unit.py` → 105 tests, OK.
- `python3 -B scripts/test-m6-clients.py --run` → passed (docker-free, `with_runtime: False`, `docker_settle: null`). Ran as attempt-4; had to temporarily move aside the pre-existing `docs/validation/M6/clients.json` (an exclusive-write current receipt from an earlier `passed`/attempt-2 run already in the tree) since the harness refuses to overwrite it, then restored the original content byte-for-byte afterward. `git status` confirms nothing else changed.

**Risks / open issues**:
- `running_analyzer_containers` relies on the `org.rust-mcp.execution=true` label being applied to every M6-spawned container; if that label is ever dropped from the analyzer gateway's `docker run` invocation, the settle would silently see zero containers and just do the flat 20s cooldown.
- The 60s timeout and 20s cooldown are fixed constants per the task's instruction; if host saturation from a heavier load needs longer, these would need tuning, not made configurable.
- Not exercised against real Docker/containers in this session — only the control flow and the docker-free path are verified; the actual settle behavior under real container teardown will only be proven when the orchestrator runs `--with-runtime`.
