# M1-16 participant fixes — bounded worker result, 2026-09-04

Task: fix participant cleanup and evidence without model, Docker or installation
runs. Principal owns architecture, broker, runner/evaluator, frozen reproduction
copies, independent disposition and requalification.

Result: implemented and locally validated. The prior 14 tests remain; 18 new tests
cover the changed boundaries. The initial filtered-feature inventory inference was
incorrect and is corrected below with actual unfiltered no-model config receipts. No utility inference or milestone closure claimed.

Files changed:
- target/m1-16-controller/participant.py
- target/m1-16-controller/test_participant.py
- target/m1-16-controller/README.md
- target/M1-16-participant-fixes.md

Tests executed:
`python3 -W error::ResourceWarning -m unittest discover -s target/m1-16-controller -p test_participant.py -v`
32/32 pass (latest run 0.968 seconds, exit 0).
`python3 -m py_compile target/m1-16-controller/participant.py target/m1-16-controller/test_participant.py`
passed before final additional test; unittest subsequently imports both final files.

Evidence:
- M3: real pipes + injected ps errors retain receipt, join actual owned child and
  readers, preserve original failure, and report inspection uncertainty. Terminate
  errors still proceed to kill/wait and joins. Cleanup codes/types retain no raw
  exception text. An unexpected close exception is caught by receipt finalization.
- Monitor observations use a lock; stop/join precedes the snapshot. A delayed-ps
  fixture verifies monitor completion before final inspection. PID birth identity
  is compared when checking remaining observations. Normal streams explicitly close.
- M6: encoded-byte FIFO with a condition and 16 MiB capacity replaces queue32.
  A real pipe emits 100 queued notifications while consumption pauses beyond the
  former .2-second timeout; all 100 arrive without terminal queue failure. Separate
  byte-pressure test verifies consume/close wakeups. Cumulative stdout remains16MiB.
- M4/H1: existing receipt/events/neutral reject before launch. Actual cwd is now a
  private fresh /private/tmp/m1-16-neutral-* directory, with JSON path marker at
  output_dir/neutral. Empty directory removal requires verified joins/absence;
  nonempty or uncertain directories remain, recorded. Tests check mode0700,
  external parent, preservation and reuse rejection.
- H3 correction: the original preflight.py deliberately filtered effective_features,
  so that receipt was not an exact inventory. Principal's resumed smoke stopped
  safely before any model turn. Follow-up actual config/read captures below use
  no filtering of keys and no thread/start or turn/start. The first bool-only
  assertion failed on network_proxy:null and its joined cleanup was retained;
  the shape-aware capture then established all41keys. Six additional boolean
  flags are explicitly false after pinned-source review; network_proxy remains
  null without any override. Final actual unfiltered map equals EXPECTED_FEATURES,
  all generic guards pass, native MCPs disabled, child exits0 and all observed PIDs
  absent. This is serialized config, NOT runtime defaults/native tool inventory.
- H6/L1/L3/L5: prompt/tool declarations hashes; effective feature hash/key list;
  sanitized preflight event sequence; task_status survives cleanup status; separate
  cleanup_failed/infrastructure_failed; first stop_reason and subsequent reasons.
- M1/L7: handler result after cancellation stays in event evidence with
  cancellation_after_handler=true. Broker candidate receipt remains authoritative
  even if delivery subsequently fails. Known cancellation RuntimeError codes
  (cancelled, driver_cancelled_or_deadline) only classify cooperative cancellation
  when event was already set; other failures retain type without arbitrary text.
  Retryable broker-error responses use success:false without interruption.

Receipt/interface additions:
- prompt_sha256, tool_declarations_sha256, effective_features_sha256,
  effective_feature_keys, feature_guard_scope;
- neutral_cwd, neutral_directory_disposition; output_dir/neutral is now a JSON marker;
- task_status, cleanup_failed, infrastructure_failed, stop_reason, stop_reasons;
- cleanup.parent_joined, inspection_complete, cleanup_errors;
  remaining_observed_pids=null means unknown, never proof of absence;
- successful call cancellation_after_handler; raised call cancellation_before_failure,
  failure_class and fixed failure_code only for known cooperative cancellation.
Legacy status remains unchanged in shape, including cleanup_failed on failure.

Risks:
- Binary/source pins remain mandatory in runner. The reviewed no-environment narrow
  V8 API is the confinement basis; it is not an OS sandbox or hard native RAM/CPU bound.
- App-server startup failure before Transport construction completes is still a
  runner setup failure; run_participant does not promise a receipt when process
  creation/configuration itself cannot complete. Fresh neutral marker permits
  identifying that path, and reuse rejects it.
- OS process inspection failure cannot establish absence or justify signalling an
  unverified PID; it fails closed while attempting joins of known owned parent.
- Native exact-model requalification was intentionally not run. Updated controller
  hashes and canonical reproduction-copy preservation belong to principal.
- The shared output-policy issue H4 and authoritative committed-candidate handling
  are broker/runner integration concerns; participant response and transport caps
  remain enforced and require joint requalification.

Decisions: preserved exact model/effort, supported authentication, no-environment
thread/turn restrictions, no other MCPs/effect tools, existing synchronous handler
contract, bounded writes/watchdog and retryable extra-argument behavior. No product
Rust code, public MCP contract, dependencies or foundational ADR changed.

Open issues: principal diff review, joint controller tests, canonical copy update,
freeze binary/script hash refresh and independent security disposition/requalification.

## Follow-up: actual unfiltered preflight before policy correction

```json
{
  "purpose": "unfiltered feature config preflight; no thread/start or turn/start",
  "model_turns_sent": 0,
  "cli_sha256": "a29d9e86eef88cbbd69f97ce8c590b1d0a287c8f77424f5eef226b883d7eaa22",
  "cleanup": {
    "exit_code": 0,
    "parent_joined": true,
    "forced_parent_stop": false,
    "forced_owned_host_stop": false,
    "observed_processes": [
      {
        "pid": 2212,
        "ppid": 2207,
        "pgid": 2207,
        "name": "codex",
        "started": "Fri Sep 4 12:23:15 2026"
      }
    ],
    "remaining_observed_pids": [],
    "reader_joined": true,
    "inspection_complete": true,
    "cleanup_errors": [],
    "stderr_bytes": 0,
    "stderr_prefix_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "transport_failure": null
  }
}
```

## Actual unfiltered config feature shapes before correction

```json
{
  "purpose": "unfiltered feature keys, boolean values and typed value shapes only; no thread/start or turn/start",
  "model_turns_sent": 0,
  "cli_sha256": "a29d9e86eef88cbbd69f97ce8c590b1d0a287c8f77424f5eef226b883d7eaa22",
  "features": {
    "apps": false,
    "artifact": false,
    "auth_elicitation": true,
    "background_paginated_rollout_migration": false,
    "browser_use": false,
    "browser_use_external": false,
    "code_mode": false,
    "code_mode_host": true,
    "code_mode_only": false,
    "computer_use": false,
    "current_time_reminder": false,
    "deferred_executor": false,
    "deferred_tool_world_state": false,
    "goals": false,
    "hooks": false,
    "image_generation": false,
    "in_app_browser": false,
    "in_app_chat": false,
    "in_app_dictation": false,
    "in_app_local_automation": false,
    "js_repl": false,
    "mcp_2026_07_28": false,
    "memories": false,
    "mentions_v2": true,
    "multi_agent": false,
    "multi_agent_v2": false,
    "network_proxy": null,
    "plugins": false,
    "realtime_conversation": false,
    "remote_control": false,
    "remote_plugin": false,
    "shell_tool": false,
    "skill_mcp_dependency_install": false,
    "skill_search": false,
    "skip_host_skill_discovery": true,
    "sleep_tool": false,
    "standalone_web_search": false,
    "token_budget": false,
    "tool_suggest": false,
    "view_image": false,
    "workspace_dependencies": false
  },
  "features_raw_sha256": "620eb0166948c494a5d1b512619cde13ecbd3d6e21d5689f587e7c6a5bc1b22c",
  "cleanup": {
    "exit_code": 0,
    "parent_joined": true,
    "forced_parent_stop": false,
    "forced_owned_host_stop": false,
    "observed_processes": [
      {
        "pid": 2279,
        "ppid": 2274,
        "pgid": 2274,
        "name": "codex",
        "started": "Fri Sep 4 12:23:40 2026"
      }
    ],
    "remaining_observed_pids": [],
    "reader_joined": true,
    "inspection_complete": true,
    "cleanup_errors": [],
    "stderr_bytes": 0,
    "stderr_prefix_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "transport_failure": null
  }
}
```

## Final exact no-model preflight after source-reviewed overrides

```json
{
  "purpose": "source-reviewed exact serialized config preflight; no thread/start or turn/start",
  "utc": "2026-09-04T17:25:16.347941+00:00",
  "model_turns_sent": 0,
  "threads_started": 0,
  "cli_sha256": "a29d9e86eef88cbbd69f97ce8c590b1d0a287c8f77424f5eef226b883d7eaa22",
  "source_hashes": {
    "features.rs": "b7ef7bb1bb5517a82ae7c7294f2d7a072cb00f151a3d8f5025aa2e967b036937",
    "config.rs": "b3eb0a9751522f7f6f7a7bd1b21253f2fcb4e669955819bfb6d0a7425e1fed9e"
  },
  "exact_feature_config": true,
  "effective_features_unfiltered": {
    "apps": false,
    "artifact": false,
    "auth_elicitation": false,
    "background_paginated_rollout_migration": false,
    "browser_use": false,
    "browser_use_external": false,
    "code_mode": false,
    "code_mode_host": true,
    "code_mode_only": false,
    "computer_use": false,
    "current_time_reminder": false,
    "deferred_executor": false,
    "deferred_tool_world_state": false,
    "goals": false,
    "hooks": false,
    "image_generation": false,
    "in_app_browser": false,
    "in_app_chat": false,
    "in_app_dictation": false,
    "in_app_local_automation": false,
    "js_repl": false,
    "mcp_2026_07_28": false,
    "memories": false,
    "mentions_v2": false,
    "multi_agent": false,
    "multi_agent_v2": false,
    "network_proxy": null,
    "plugins": false,
    "realtime_conversation": false,
    "remote_control": false,
    "remote_plugin": false,
    "shell_tool": false,
    "skill_mcp_dependency_install": false,
    "skill_search": false,
    "skip_host_skill_discovery": true,
    "sleep_tool": false,
    "standalone_web_search": false,
    "token_budget": false,
    "tool_suggest": false,
    "view_image": false,
    "workspace_dependencies": false
  },
  "effective_feature_count": 41,
  "effective_features_sha256": "61ff4008082b4085cc0dfefc02505ee8259789d649fcc9f3e07fde0d8864b2ed",
  "guard_checks": {
    "agents.enabled": true,
    "orchestrator.skills.enabled": true,
    "orchestrator.mcp.enabled": true,
    "skills.include_instructions": true,
    "skills.bundled.enabled": true
  },
  "mcp_disabled": true,
  "cleanup": {
    "exit_code": 0,
    "parent_joined": true,
    "forced_parent_stop": false,
    "forced_owned_host_stop": false,
    "observed_processes": [
      {
        "pid": 2780,
        "ppid": 2775,
        "pgid": 2775,
        "name": "codex",
        "started": "Fri Sep 4 12:25:16 2026"
      }
    ],
    "remaining_observed_pids": [],
    "reader_joined": true,
    "inspection_complete": true,
    "cleanup_errors": [],
    "stderr_bytes": 0,
    "stderr_prefix_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "transport_failure": null
  }
}
```

## Source dispositions for the seven previously unaccounted keys

Pinned local Codex0.153.0 source hashes are recorded in final preflight above.
No source was guessed from the key name:

| Key | Pinned source evidence | Decision |
| --- | --- | --- |
| auth_elicitation | features.rs324-325 and1586-1591; config.rs1776-1784 selects MCP URL/form elicitation capability | Explicit false: no connector-auth prompting; normal supported model login/auth configuration unchanged. |
| background_paginated_rollout_migration | features.rs180-181,1103-1107: migration of local legacy rollout files | Explicit false: no unrelated local migration. |
| js_repl | features.rs355-358,574-579,971-975: removed compatibility no-op | Explicit false; no claim that it disables the separate code-mode host. |
| remote_control | features.rs386-387,580-582,1629-1633: removed compatibility no-op | Explicit false. |
| mentions_v2 | features.rs294-295,1479-1483: TUI mention popup | Explicit false; participant uses app-server with no interactive mention UI. |
| mcp_2026_07_28 | features.rs206-207,1263-1267; config.rs1798-1804 selects native Codex MCP protocol mode | Explicit false: no native MCP integrations admitted. The separate product rmcp3.2.0 driver and its MCP protocol remain unchanged. |
| network_proxy | features.rs186-189,777 optional typed field,1205-1213 default false; config.rs3621-3630 activates only under its permission conditions | Preserve observed null and add no override. Any nonnull value fails this freeze; do not deactivate host security policy. |

Actual final serialized config is41keys:38false booleans, code_mode_host=true,
skip_host_skill_discovery=true, network_proxy=null. Hash over actual reply-order
compact serialization:61ff4008082b4085cc0dfefc02505ee8259789d649fcc9f3e07fde0d8864b2ed.
This does not claim that unconfigured/default feature keys cannot enable behavior;
immutable source/binary review and no-environment confinement remain necessary.

Validation additions reject connector elicitation re-enabled, reject proxy drift
without overriding it, and assert explicit false overrides for the six source-
reviewed flags. No Docker, no model call, no thread creation or install occurred
in this follow-up. Parent must run the exact-model requalification and refresh
controller hashes/reproduction copies after reviewing this correction.

## Opus follow-up P2 — buffered descriptor ownership, 2026-09-04

Removed the last-resort `os.close(stream.fileno())` path. A BufferedReader owns its
file descriptor; externally closing the borrowed number could let a subsequent
BufferedReader.close/destructor close a different descriptor after OS reuse.

After bounded joins, a surviving daemon reader now produces `reader_join_timeout`;
its bound target retains Transport and stream ownership while it remains alive.
No borrowed descriptor is closed, detached or replaced. Streams close normally only
when all readers have joined. Parent and identity-verified host cleanup attempts
still precede reader joins. `reader_joined:false` and `inspection_complete:false`
remain cleanup failure, so the runner must stop the series.

Discriminating real-pipe test keeps the writer open so the BufferedReader remains
blocked. `Transport.close()` returns within2.5s, reports joined parent/unjoined reader
and inspection uncertainty, never calls os.close, and leaves the same descriptor
open and owned by the reader. Test teardown alone closes its own pipe writer to
release EOF, joins the reader and then closes the stream owner.

Validation: `python3 -W error::ResourceWarning -m unittest discover -s
 target/m1-16-controller -p test_participant.py -v`:33/33 pass,1.925s. Only participant.py,
test_participant.py and this append changed. No model, Docker, corpus, runner,
evaluator or broader configuration change. Parent requalification/copy/hash refresh
remains required before freeze.
