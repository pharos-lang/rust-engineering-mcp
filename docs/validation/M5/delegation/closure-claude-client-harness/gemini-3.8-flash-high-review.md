## Verdict

**Block**

The qualification harness contains two P2 oracle strictness issues: (1) it allows sessions that degraded or fell back to another model to pass despite the harness specification declaring fallback a failure, and (2) it leaves `list_mcp_resources` unconstrained and unverified in the Docker-free flow. Additionally, several P3 issues affect portability across environments, error handling, and test coverage.

---

## Findings

| Severity | File:line or function | Finding | Evidence from the diff |
| :--- | :--- | :--- | :--- |
| **P2** | `scripts/test-m5-clients.py:1258–1260` (`validate_claude_session`) | **Acceptance of fallback models**: The oracle checks only that `CLAUDE_MODEL` is present in `modelUsage`, not that unauthorized or fallback models are absent. If Claude Code experiences a fallback mid-session, both models appear in `modelUsage` and the turn is accepted. | Lines 71–73 specify: *"Version and model are pinned: the session's own `init` event must report both, and a fallback to another model is a failure."* In contrast, lines 1258–1260 only check `if not isinstance(usage, dict) or CLAUDE_MODEL not in usage: raise RuntimeError(...)`. Any additional fallback model present in `usage` (e.g. Opus or another Sonnet variant) is ignored and reported as valid in `session["observed_models"]`. |
| **P2** | `scripts/test-m5-clients.py:1320–1345` (`validate_docker_free_model_flow`) | **Unconstrained and unverified `list_mcp_resources` in Docker-free flow**: `list_mcp_resources` is included in `allowed`, but its occurrence count, arguments, and outcome status are never validated. | Line 1323 sets `allowed = {"rust.project.open", "list_mcp_resources", *M5_TOOLS}`. Lines 1335–1345 only iterate over `for tool in M5_TOOLS:`. Unlike `validate_runtime_model_flow` (lines 1360–1368, which strictly checks `len(discoveries) == 1`, arguments, and completion status), `validate_docker_free_model_flow` allows `list_mcp_resources` to be omitted, repeated indefinitely (e.g. in a retry loop), or failed with error without triggering an oracle failure. |
| **P3** | `scripts/test-m5-clients.py:1330–1340` (`validate_docker_free_model_flow`) | **Ordering gap across roots and refusals in Docker-free flow**: The oracle does not enforce relative ordering between planned tool calls, nor does it enforce that all project roots are opened before tool calls begin. | Lines 1338–1339 check only that `completed.index(call) < opens[str(ROOT / FIXTURES[row["project"]])]`. While the prompt instructs the model to open all roots in step (1) before executing steps (2)–(5) in order, the validator permits tool calls to be interleaved with project opens or executed in an arbitrary order. |
| **P3** | `scripts/test-m5-clients.py:60, 1604` (`CLAUDE`, `claude_environment`) | **Host-specific user path hardcoding**: `CLAUDE` and `PATH` hardcode `/Users/cburgosro`, preventing execution on other host accounts or CI environments. | Line 60 defines `CLAUDE = pathlib.Path("/Users/cburgosro/.local/bin/claude")` and line 1604 prepends `PATH` with `/Users/cburgosro/.local/bin`. Previously, Codex was resolved via `shutil.which("codex")`. Preflight precondition `claude_binary` (line 785) will fail on any environment where the user path differs. |
| **P3** | `scripts/test-m5-clients.py:1664` (`claude_gate`) | **Unhandled `json.JSONDecodeError` on non-JSON stdout lines**: Stream-json parsing assumes every non-empty line of stdout is valid JSON. | Line 1664 parses stdout with `events = [json.loads(line) for line in outcome["stdout"].splitlines() if line.strip()]`. If Claude Code or Node prints any non-JSON startup banner, deprecation warning, or diagnostic line to stdout while exiting with code 0, `json.loads` raises an unhandled `json.JSONDecodeError` instead of a structured gate error. |
| **P3** | `scripts/test-m5-clients-unit.py:415, 439` (`ClaudeTranscriptTests`, `ModelDirectedDockerFreeTests`) | **Unit test gaps for fallback models, discovery retries, and docker-free ordering**: The unit tests do not assert rejection of transcripts exhibiting these edge cases. | Line 430 in `test_session_must_be_the_pinned_client_and_model_on_the_configured_server` only tests when `CLAUDE_MODEL` is absent from `usage`, not when a fallback model coexists with `CLAUDE_MODEL`. In `ModelDirectedDockerFreeTests` (lines 439–470), there are no tests for duplicate `list_mcp_resources` calls or out-of-order execution. |

---

## What I verified

### 1. Security & Hygiene
- **Credential Handling**: Verified that no credentials or authentication tokens are copied, symlinked, or staged under `/private/tmp` or the `attempt` directory. `claude_logged_in()` reads `claude auth status` in place (lines 704–714), and `claude_gate` sets `"credentials_copied": False` (line 1678).
- **Environment Isolation**: Verified `claude_environment` (lines 1600–1614) passes an explicit, minimal set of environment variables (`HOME`, `PATH`, `TMPDIR`, `LANG`, `MCP_TOOL_TIMEOUT`, `USER`, `LOGNAME`, `SHELL`), isolates `TMPDIR` to `private / "tmp"`, runs with `cwd` at `private / "cwd"`, and does not pass through external API keys.
- **Client Restrictions**: Verified `claude_argv` (lines 1650–1659) enforces `--restricted`, `--setting-sources ""`, `--strict-mcp-config`, `--disable-slash-commands`, `--no-chrome`, `--permission-mode dontAsk`, `--permission-prompts none`, `--tools ListMcpResourcesTool,ReadMcpResourceTool`, and `--allowedTools mcp__rust_engineering,ListMcpResourcesTool,ReadMcpResourceTool`.
- **Capability Lockdown**: Verified `validate_claude_session` (lines 1249–1257) checks that `init.tools` advertises only `CLAUDE_RESOURCE_TOOLS` or tools prefixed with `mcp__rust_engineering__`, and asserts `final.permission_denials` is empty.
- **Evidence Staging**: Verified that raw events (`claude-{mode}-model-events.jsonl`) and stderr are written directly to `attempt` without leaking temporary directory paths, and `claude_argv` is recorded solely as a SHA-256 digest (`argv_sha256`, line 1682).

### 2. Oracle Strictness
- **Runtime Flow Strictness**: Verified that `validate_runtime_model_flow` (lines 1348–1428) strictly enforces:
  - Exactly 7 completed MCP calls: 1 `rust.project.open`, 1 `list_mcp_resources`, 2 `rust.benchmark.run`, 2 `rust.benchmark.compare`, and 1 `read_mcp_resource`. Any fewer or additional calls fail via count or tool allowlist checks.
  - Strict sequence verification via `positions == sorted(positions)` (lines 1414–1418), ensuring open $\to$ discovery $\to$ run 1 $\to$ run 2 $\to$ positive compare $\to$ negative compare $\to$ resource read.
  - Exactly-once dataset binding: baseline and candidate dataset IDs are captured directly from the two live runs via `check_runtime_observation` (lines 1380–1385), and positive comparison data must match both dynamic IDs (lines 1400–1403).
  - Anti-forgery on non-dataset comparison: the negative comparison must target a real non-dataset artifact emitted during the session (lines 1405–1411), returning declared status `blocked` and error code `NOT_A_DATASET`.
  - Resource read binding: `validate_model_resource_read` (lines 1290–1309) verifies that the exact URI corresponding to the compared non-dataset artifact is read natively, returning completed status and non-empty `text` or `blob`.
- **Docker-free Flow Validation**: Verified that `validate_docker_free_model_flow` (lines 1320–1345) checks that all four M5 tools are called exactly once with the planned arguments, checks status and error codes against the plan, and verifies each tool is called after opening its planned root.

### 3. Evidence Integrity
- **Plan Row Binding & Receipt Fields**: Verified that `CALL_CLIENTS = frozenset({"inspector"})` (line 410) correctly mirrors that only the deterministic Inspector converts all plan rows, while Claude Code receipts (`receipt["claude_code"][mode]`, lines 1675–1693) record `model_flow`, `session`, `duration_seconds`, `tool_calls`, `artifact_resources_read`, `model_events_sha256`, and `stderr_sha256` without fabricating row conversion entries.
- **Inventory & Source Hashes**: Verified that `source_hashes()` (lines 428–430) and `PreflightTests` (lines 830–836) correctly removed `docs/validation/M1/17-codex-client/controller.py` from tracked gate inputs and accurately track unit tests and session drivers.
- **Finding status for Goal 3**: No findings. Evidence fields, SHA-256 digests, and client version records accurately reflect the verified actions.

### 4. Process Control & Python Implementation
- **Process Management**: Verified `run_claude` (lines 1617–1633) uses `start_new_session=True` and terminates the entire process group with `os.killpg(child.pid, signal.SIGKILL)` upon timeout before reading output.
- **Temporary State Cleanup**: Verified `private` directory created under `/private/tmp` with `0o700` permissions is reliably cleaned up in `finally: shutil.rmtree(private, ignore_errors=True)` (line 1695).
- **Payload Parsing**: Verified `claude_structured_payload` (lines 1134–1152) safely parses JSON strings and text content block arrays, returning a dictionary only when exactly one candidate carrying `"status"` is found.
