# Package R00 — Independent research: SEP-2663 Tasks support in rmcp 3.2.0 (read-only)

You are Gemini 3.8 Flash High invoked via `agy` by the orchestrator (Claude Fable 5.1) of the Rust Engineering MCP project. Role: independent read-only researcher. Do not modify any file. Do not run cargo, network, or installers. If your sandbox prevents reading a file, say so explicitly rather than guessing.

## Package contents
- `rmcp-3.2.0/` — verbatim source of the `rmcp` crate version 3.2.0 exactly as locked in the project's Cargo.lock (SHA-256 of rmcp-3.2.0/Cargo.toml: 0ad47897f5429df21ae73d743563b2ffd03d58453a3b0ea4364f97e3d3b8e362). This is the ONLY authoritative source. Do not rely on memory of other rmcp versions; if you cite documentation from memory, label it "unverified".

## Questions (answer each with file path + line numbers from this package; write "NOT FOUND in rmcp 3.2.0" when absent)
1. Protocol versions: list every `ProtocolVersion` constant and the default/latest negotiated version. Confirm whether `2024-11-05`, `2025-03-26`, `2025-06-18`, `2025-11-25`, `2026-07-28` all exist.
2. Tasks extension (SEP-2663, `io.modelcontextprotocol/tasks`): which request methods exist (`tasks/get`, `tasks/result`, `tasks/cancel`, `tasks/list`, `tasks/update`, others), their param/result types, the `task` field in `CallToolRequestParams` / `_meta` (TaskAugmentedRequestParamsMeta), `CreateTaskResult`, task status enum values and terminal states, `ttl`/`pollInterval` fields, `notifications/tasks/status` if any.
3. Capability negotiation: how a server declares task support (`ServerCapabilities` fields, e.g. `tasks` with `requests.tools.call`), how a client declares it, and whether rmcp gates task methods on the negotiated protocol version.
4. Server-side integration: what `ServerHandler` methods exist for tasks (`get_task`, `get_task_result`, `cancel_task`, `list_task`, ...), their default implementations (do they return MethodNotFound?), and what `src/task_manager.rs` provides (types, storage, expiry, cancellation semantics, whether it is in-memory, whether it spawns Tokio tasks, whether it is feature-gated — list Cargo features involved).
5. Cancellation: how request cancellation (`notifications/cancelled`) reaches a handler (CancellationToken in `RequestContext`), how it interacts with tasks, and whether `tasks/cancel` triggers the token.
6. Result delivery: for a tool call executed as a task, how the final `CallToolResult` is returned (`tasks/result` blocking? polling `tasks/get` until terminal?), and whether an error result versus JSON-RPC error is distinguished.
7. Anything in this version that looks experimental, TODO, or partially implemented in the task code path (grep for `todo!`, `unimplemented!`, `TODO`, `FIXME`, `experimental`).
8. Wire examples: reconstruct from the types the exact JSON of (a) a `tools/call` request that asks for task execution, (b) a `CreateTaskResult` response, (c) a `tasks/get` request/response, (d) `tasks/result` request/response, (e) `tasks/cancel`. Mark each field as required/optional per the Rust type.

## Delivery format
Markdown report with sections: Summary (5 lines max), Findings per question with citations `path:line`, Contradictions/uncertainties, Discriminating tests the project should write to prove wire compatibility (at least 6, one line each), Limitations of this review (what you could not read or execute). No recommendations about product architecture beyond what the code shows.
