# Participant client: infrastructure only

`run_participant(prompt, dynamic_tools, handler, output_dir, wall_seconds=900,
max_output_tokens=30000)` returns and writes a sanitized receipt. The handler is
called as `handler(name, arguments, cancellation_event)`, synchronously on the
caller thread. It must propagate cancellation through the real broker/gateway and
wait for its cleanup. A watchdog sets the event and sends `turn/interrupt` even
while the handler is blocked. Uncooperative handler code cannot be safely killed
by Python; the client does not detach handler work.

Dynamic names must be unique with a closed object root schema. The participant
admits only named tools and object arguments; the broker validates required/extra
fields, nested types and domain operations and returns retryable denials. Those
denials use `success:false` and do not interrupt the participant. Unadmitted native
requests fail closed. There are at most 64 admitted dynamic calls; the parent
broker counts compound validations appropriately.

Stdout JSON lines are limited to 1 MiB, cumulative received stdout to 16 MiB.
The condition-based FIFO retains encoded messages with a 16 MiB byte capacity;
it applies backpressure without an arbitrary message-count timeout during a
synchronous handler. Closing the FIFO wakes blocked producers. Event log plus
receipt are bounded to 16 MiB. Stderr prefix (64 KiB) is retained only in memory
and hashed; raw stderr is never stored. Stderr beyond 16 MiB stops admission.
Bodies live once in `events.jsonl`; receipts retain hashes, lengths and timing.
RPC preflight events record sequencing without configuration/auth response bodies.
Prompt bytes and ordered tool declarations are hashed in each receipt.

An existing `receipt.json`, `events.jsonl` or `neutral` rejects reuse before launch.
The app-server cwd is a fresh mode-0700 `m1-16-neutral-*` directory in `/private/tmp`,
separate from product/corpus/evidence ancestors. `output_dir/neutral` is a JSON
marker recording that path. Only an empty cwd after verified parent/reader joins
and process absence is removed. Nonempty or uncertain directories are preserved,
with their path and disposition in the receipt; cleanup never recursively deletes.

Model: exact `gpt-5.6-sol`/medium, fallback false. No thread environments, workspace
roots, selected capability roots or turn environment overrides. All configured
native MCP integrations are disabled and checked through effective config.
Configuration-file MCP name scanning only supplies overrides; effective config is
the check, including managed/project layers.

The exact serialized feature config is frozen from an unfiltered no-model
`config/read`, with source-reviewed overrides. The older preflight script filtered
its `effective_features` and was not a complete inventory. The follow-up in
`target/M1-16-participant-fixes.md` records this correction and actual maps/hashes.
`code_mode_host` and `skip_host_skill_discovery` must be true; `DISABLED` keys must
be false, including connector auth elicitation, background rollout migration,
legacy JS/remote-control no-ops, native MCP protocol selection and TUI mentions.
Connector elicitation controls do not change normal supported login credentials.
`network_proxy` must match the observed null optional field: it is not overridden;
a different host security setting causes rejection, not silent deactivation.
Unknown keys (including false) and changed values fail before the model turn.
This map does not enumerate enabled defaults or native tools. The runner must also
pin installed Codex 0.153.0/code-mode-host binaries and source evidence. The
source-reviewed narrow V8 API is the confinement claim; it is not an OS sandbox
or a hard native CPU/RAM bound.

Process observations use a lock and the monitor is stopped/joined before final
inspection. Cleanup phase errors retain sanitized codes/types and still attempt
owned parent/reader joins. `inspection_complete:false` or
`remaining_observed_pids:null` means process absence is unproved. Only a previously
observed code-mode host with matching birth time, name, original parent, own group
and current parent/orphan relationship can be signalled. Forced stops, failed
inspection and unjoined readers remain cleanup failures. Normal joined streams
are explicitly closed.

`task_status` and `turn_status` survive infrastructure/cleanup failure. The legacy
`status` still reports `cleanup_failed` or `receipt_budget_exceeded` when needed;
`cleanup_failed` and `infrastructure_failed` are separate flags. The first
`stop_reason` and subsequent `stop_reasons` distinguish wall, output, admission and
handler failures. Arbitrary handler/cleanup exception messages are not retained.
Known cooperative cancellation codes are classified separately when cancellation
was already set. A handler result returned after cancellation remains in events,
with `cancellation_after_handler:true`; its committed candidate is not discarded.
The broker owns the authoritative candidate list even if response delivery fails.

Token updates replace cumulative totals; missing usage is unknown. The output
threshold observes `total.outputTokens`; it is not a hard model cap. Wall expiry
stops admission, while synchronous handler/native cleanup can extend elapsed time.

Validation: `python3 -W error::ResourceWarning -m unittest discover -s
target/m1-16-controller -p test_participant.py -v` passes 32 tests, including real
pipes and child joins, fake RPC/ps failures, cancellation races, feature rejection,
fresh output and neutral-cwd preservation. No model, Docker, installation or utility
run was executed for these fixes. Prior exact-model echo/timeout receipts remain
historical qualification evidence and require principal requalification of this diff.
