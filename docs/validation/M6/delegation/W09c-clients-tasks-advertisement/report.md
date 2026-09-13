# W09c — Tasks advertisement: alinear el arnés M6 con el server real

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort high --disallowedTools Agent Task`) |
| Inicio / fin (UTC) | 2026-09-13T05:18:55Z / 2026-09-13T05:23:32Z |
| Resultado | `--run` **verde**: 5/5 docker-free SANDBOX_DENIED (Inspector+Claude Code), `tasks_advertised: true`, socket ausente, credential scan clean |

---

## Report

**Task:** Align the M6 client harness's protocol-level Tasks negotiation with the M5 precedent (server advertises Tasks; only the five analyzer tools carry no per-tool Tasks/Resource surface), fixing the `--run` failure at `validate_protocol_metadata`.

**Result:** Root cause confirmed exactly as diagnosed — the stock server (36 tools) advertises the Tasks capability at the protocol level because other tool families (coverage/mutation/benchmark) use it, but the M6 harness was asserting `expected_advertised=False`. Fixed by two edits to `scripts/test-m6-clients.py`; `--run` now completes green.

**Files changed:**
- `scripts/test-m6-clients.py`:
  - `validate_protocol_metadata` (L666): `m3.protocol_summary(path, False)` → `m3.protocol_summary(path, True)`.
  - `client_versions()` (L525): `inspector.tasks` → `True`; `claude_code.tasks` stays `False` (matches M5's own inspector/claude_code split, driven by each client's own `clientCapabilities` declaration, not the server). Added a one-line docstring explaining the `resource` divergence from M5.
- `scripts/test-m6-clients-unit.py`: no changes needed — verified none of the 96 existing unit tests assert the old `False` value or a `server/discover` row shape that the `True` change would break (`test_metadata_validator_accepts_the_m3_proxy_shape` and the `RunOrchestrationTests` fake never set `method: "server/discover"`, so `protocol_summary`'s modern-session branch never triggers there).

**Decision on `resource`:** Left `False` for both clients, diverging from M5's `True`. M6's own session driver (`m6-inspector-session.mjs`) explicitly documents "none of these five tools publish an MCP Resource, so no Resource oracle runs here," and `inspector_gate`/`validate_call_rows` in `test-m6-clients.py` never read or assert `outcome.get("resource")` (unlike M5's `inspector_gate`, which requires `outcome.get("resource") is True`). Since M5's `True` reflects that M5's tools *do* publish artifact Resources and the M5 oracle checks it, and neither is true for M6, forcing `True` here would misrepresent reality with no oracle backing it. `tasks`, by contrast, is what actually differs by tool-family composition of the *whole server*, which is why M5's own value (`True`/`False` split) transfers directly.

**Tests:**
```
python3 -B scripts/test-m6-clients-unit.py
Ran 96 tests in 2.862s
OK
```
```
python3 -B scripts/test-m6-clients.py --run
```
Completed successfully (exit 0, no traceback), wrote `docs/validation/M6/clients.json` with:
- `"status": "passed"`
- 5/5 Docker-free rows `unavailable`/`SANDBOX_DENIED` for both Inspector and Claude Code (`model_flow.refusals` all five tools, `SANDBOX_DENIED`)
- `"docker_free_socket_created": false`, `"private_directory_removed": true`, `"evidence_credential_scan": "clean"`
- `protocol.clients.inspector.tasks_advertised == [true]`, `protocol.clients.claude-code.tasks_advertised == [true]` — the fix under test
- `protocol.clients.inspector.tasks_declared == [true]`, `protocol.clients.claude-code.tasks_declared == [false]` — unchanged, matches each client's own declared capability

**Risks:** None identified; the change only widens what the oracle expects the server to advertise (matching reality) and doesn't touch the fingerprint fix, argv construction, or runtime matrix.

**Open issues:** None. No further pre-existing defect surfaced once Tasks was corrected — `--run` went green on the first attempt after the fix.

No commits were made, per instructions.
