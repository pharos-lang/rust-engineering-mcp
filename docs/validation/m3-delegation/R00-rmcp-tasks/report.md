# Independent Research Report: SEP-2663 Tasks Support in `rmcp` 3.2.0

## Summary
`rmcp` 3.2.0 implements SEP-2663 tasks via the `io.modelcontextprotocol/tasks` extension under `feature = "server"`, exposing [`TaskManager`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L300-L538), [`TaskStatus`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L17-L35), [`CreateTaskResult`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L278-L322), and methods `tasks/get`, `tasks/update`, and `tasks/cancel`. Task execution is initiated exclusively by a server returning [`CreateTaskResult`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L278-L322) from `tools/call` when the client declares the extension capability, not via a `task` request parameter. Methods `tasks/result` and `tasks/list` do not exist; clients retrieve results by polling `tasks/get`. Cancellation is cooperative through [`TaskContext`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L53-L147) and independent of transport JSON-RPC [`CancellationToken`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/service.rs#L1204) in [`RequestContext`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/service.rs#L1200-L1223). Task methods are gated strictly on capability advertisement, not on the negotiated protocol version.

---

## Findings per Question

### 1. Protocol Versions
- **Constants**:
  - `V_2024_11_05 = ProtocolVersion(Cow::Borrowed("2024-11-05"))` ([`src/model.rs:174`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L174))
  - `V_2025_03_26 = ProtocolVersion(Cow::Borrowed("2025-03-26"))` ([`src/model.rs:173`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L173))
  - `V_2025_06_18 = ProtocolVersion(Cow::Borrowed("2025-06-18"))` ([`src/model.rs:172`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L172))
  - `V_2025_11_25 = ProtocolVersion(Cow::Borrowed("2025-11-25"))` ([`src/model.rs:171`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L171))
  - `V_2026_07_28 = ProtocolVersion(Cow::Borrowed("2026-07-28"))` ([`src/model.rs:170`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L170))
  - `LATEST = Self::V_2025_11_25` ([`src/model.rs:175`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L175))
  - `STANDARD_HEADERS = Self::V_2026_07_28` ([`src/model.rs:178`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L178))
  - `KNOWN_VERSIONS = &[Self::V_2024_11_05, Self::V_2025_03_26, Self::V_2025_06_18, Self::V_2025_11_25, Self::V_2026_07_28]` ([`src/model.rs:181-187`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L181-L187))
- **Default and Latest Negotiated Version**:
  - `ProtocolVersion::default()` evaluates to `Self::LATEST`, which is `ProtocolVersion::V_2025_11_25` (`"2025-11-25"`) ([`src/model.rs:157-161`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L157-L161)).
  - For server `initialize` handshake negotiation, `2026-07-28` replaced `initialize` with per-request metadata (`discover` / `_meta`), defined via `is_legacy_version`: `version.as_str() < ProtocolVersion::V_2026_07_28.as_str()` ([`src/service.rs:198-200`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/service.rs#L198-L200)).
  - If a client requests `2026-07-28` over `initialize`, `negotiate_protocol_version` falls back to `newest_legacy_version` ([`src/service/server.rs:480-517`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/service/server.rs#L480-L517)), which returns `2025-11-25`. Therefore, the latest negotiated version over the `initialize` handshake is `2025-11-25`.
- **Confirmation**: `2024-11-05`, `2025-03-26`, `2025-06-18`, `2025-11-25`, and `2026-07-28` **all exist** as constants and in deserialization mapping ([`src/model.rs:170-174, 212-216`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L170-L174)).

### 2. Tasks Extension (SEP-2663, `io.modelcontextprotocol/tasks`)
- **Extension Identifier**: `TASKS_EXTENSION_ID = "io.modelcontextprotocol/tasks"` ([`src/model/task.rs:14`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L14)).
- **Request Methods**:
  - `tasks/get` (`GetTaskMethod`): Request type [`GetTaskRequest`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4229), params [`GetTaskParams`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4235-L4240) (`task_id: String`, optional `_meta`), result [`GetTaskResult`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L346-L360) (`result_type: ResultType::COMPLETE`, flattened [`DetailedTask`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L163-L183), optional `_meta`) ([`src/model.rs:4228-4240`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4228-L4240)).
  - `tasks/update` (`UpdateTaskMethod`): Request type [`UpdateTaskRequest`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4261), params [`UpdateTaskParams`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4265-L4278) (`task_id: String`, `input_responses: InputResponses`, optional `_meta`), result [`TaskAckResult`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L377-L386) (`result_type: ResultType::COMPLETE`, optional `_meta`) serialized via `ServerResult::task_ack(())` ([`src/model.rs:4260-4278, 4569-4573`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4260-L4278)).
  - `tasks/cancel` (`CancelTaskMethod`): Request type [`CancelTaskRequest`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4300), params [`CancelTaskParams`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4302-L4311) (`task_id: String`, optional `_meta`), result [`TaskAckResult`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L377-L386) (`result_type: ResultType::COMPLETE`, optional `_meta`) serialized via `ServerResult::task_ack(())` ([`src/model.rs:4299-4311, 4569-4573`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4299-L4311)).
  - `tasks/result`: **NOT FOUND in rmcp 3.2.0**.
  - `tasks/list`: **NOT FOUND in rmcp 3.2.0**.
- **Notifications**:
  - `notifications/tasks` exists (`TaskStatusNotificationMethod`): Notification type [`TaskStatusNotification`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4383-L4384), params [`TaskStatusNotificationParams`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4341-L4350) carrying a flattened [`DetailedTask`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L163-L183) and optional `_meta` ([`src/model.rs:4334-4350`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4334-L4350)).
  - `notifications/tasks/status`: **NOT FOUND in rmcp 3.2.0** (the notification path is `notifications/tasks`).
- **`task` Field and `TaskAugmentedRequestParamsMeta`**:
  - `task` field in [`CallToolRequestParams`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4054-L4071): **NOT FOUND in rmcp 3.2.0**. [`CallToolRequestParams`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4054-L4071) defines only `meta`, `name`, `arguments`, `input_responses`, `request_state`. If a client sends a legacy `task` parameter on `tools/call`, it is silently ignored by Serde ([`tests/test_task.rs:402-410`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/tests/test_task.rs#L402-L410)).
  - `task` in `_meta` ([`RequestMetaObject`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/meta.rs#L390-L489)): **NOT FOUND in rmcp 3.2.0**.
  - `TaskAugmentedRequestParamsMeta`: **NOT FOUND in rmcp 3.2.0** (only appears in a stale doc comment on deprecated `CreateMessageRequestParams` at [`src/model.rs:2855`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L2855)).
- **`CreateTaskResult`**:
  - Struct [`CreateTaskResult`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L278-L290) has fields: `result_type: ResultType` (`"task"`), `task: Task` (flattened), and optional `meta: Option<MetaObject>` (`_meta`). Deserializer strictly enforces `result_type == "task"` ([`src/model/task.rs:312-316`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L312-L316)). Returned via [`CallToolResponse::Task`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/mrtr.rs#L112) -> [`ServerResult::CreateTaskResult`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4551).
- **Task Status Enum and Terminal States**:
  - [`TaskStatus`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L21-L34): `Working` (`"working"`, default), `InputRequired` (`"input_required"`), `Completed` (`"completed"`), `Failed` (`"failed"`), `Cancelled` (`"cancelled"`).
  - Terminal states: [`TaskStatus::is_terminal(&self)`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L38-L40) returns `true` for `Completed`, `Failed`, and `Cancelled`.
- **`ttl` and `pollInterval` Fields**:
  - On [`Task`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L64, 69):
    - `pub ttl_ms: Option<u64>`: serialized as `"ttlMs"` in camelCase; when `None`, serializes as `null` on the wire (meaning unlimited retention; not omitted) ([`src/model/task.rs:61-64, 470-477`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L61-L64)).
    - `pub poll_interval_ms: Option<u64>`: serialized as `"pollIntervalMs"` in camelCase; omitted if `None` (`skip_serializing_if = "Option::is_none"`) ([`src/model/task.rs:68-69`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L68-L69)).

### 3. Capability Negotiation
- **Server Declaration**:
  - A server declares task support through the `extensions` capability map: `capabilities.extensions["io.modelcontextprotocol/tasks"] = {}` ([`src/model/capabilities.rs:429-434`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/capabilities.rs#L429-L434)).
  - Declared via builder: `ServerCapabilities::builder().enable_tasks().build()` ([`src/model/capabilities.rs:429-434`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/capabilities.rs#L429-L434)).
  - Queried via [`ServerCapabilities::supports_tasks(&self)`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/capabilities.rs#L248-L252).
  - Field `tasks` on `ServerCapabilities` or subfield `requests.tools.call`: **NOT FOUND in rmcp 3.2.0**.
- **Client Declaration**:
  - A client declares task support in the same way via `ClientCapabilities.extensions["io.modelcontextprotocol/tasks"] = {}` ([`src/model/capabilities.rs:460-465`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/capabilities.rs#L460-L465)).
  - Declared via builder: `ClientCapabilities::builder().enable_tasks().build()`.
  - Queried via [`ClientCapabilities::supports_tasks(&self)`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/capabilities.rs#L200-L204).
- **Protocol Version Gating**:
  - `validate_tasks_capability` in [`src/handler/server.rs:31-48`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/handler/server.rs#L31-L48) performs capability checks only:
    1. If server capabilities do not have `supports_tasks()`: returns `ErrorCode::METHOD_NOT_FOUND` (`-32601`).
    2. If client capabilities do not have `supports_tasks()`: returns `ErrorCode::MISSING_REQUIRED_CLIENT_CAPABILITY` (`-32021`).
  - Dispatch also verifies that if `call_tool` returns `CallToolResponse::Task(_)`, the client must have declared tasks capability; otherwise `-32021` is returned ([`src/handler/server.rs:212-216`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/handler/server.rs#L212-L216)).
  - `rmcp` **does not gate task methods on the negotiated protocol version**. Gating is exclusively capability-based.

### 4. Server-Side Integration
- **`ServerHandler` Methods**:
  - [`get_task(&self, request: GetTaskParams, context: RequestContext<RoleServer>)`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/handler/server.rs#L561-L568)
  - [`update_task(&self, request: UpdateTaskParams, context: RequestContext<RoleServer>)`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/handler/server.rs#L572-L579)
  - [`cancel_task(&self, request: CancelTaskParams, context: RequestContext<RoleServer>)`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/handler/server.rs#L583-L590)
  - `get_task_result`: **NOT FOUND in rmcp 3.2.0**.
  - `list_task`: **NOT FOUND in rmcp 3.2.0**.
- **Default Implementations**:
  - All three default implementations return `MethodNotFound` (`-32601`):
    - `Err(McpError::method_not_found::<GetTaskMethod>())` ([`src/handler/server.rs:567`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/handler/server.rs#L567))
    - `Err(McpError::method_not_found::<UpdateTaskMethod>())` ([`src/handler/server.rs:578`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/handler/server.rs#L578))
    - `Err(McpError::method_not_found::<CancelTaskMethod>())` ([`src/handler/server.rs:589`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/handler/server.rs#L589))
- **`src/task_manager.rs` Details**:
  - **Cargo Features**: Gated on `#[cfg(feature = "server")]` in [`src/lib.rs:29-30`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/lib.rs#L29-L30).
  - **Types Provided**:
    - Constants: `DEFAULT_TASK_TTL_MS = 300_000` (5 min) ([`src/task_manager.rs:39`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L39)), `DEFAULT_POLL_INTERVAL_MS = 1_000` (1 sec) ([`src/task_manager.rs:42`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L42)).
    - Timestamp helper: [`current_timestamp() -> String`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L45-L47).
    - Context: [`TaskContext`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L53-L147) (`task_id`, `request_input`, `set_status_message`, `is_cancel_requested`, `cancelled`).
    - Exit enum: [`TaskExit`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L155-L163) (`Cancelled`, `Error(McpError)`).
    - Future alias: [`TaskFuture`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L171) (`Pin<Box<dyn Future<Output = Result<CallToolResult, TaskExit>> + Send>>`).
    - Options: [`TaskOptions`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L229-L270) (`ttl_ms`, `poll_interval_ms`, `status_message`).
    - Manager: [`TaskManager`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L300-L538) (`new`, `spawn`, `get_task`, `update_task`, `cancel_task`, `running_task_count`, `shutdown`).
  - **Storage**: **In-memory only**. State is held in `Arc<Mutex<TaskManagerInner>>` with a standard `HashMap<String, TaskEntry>` ([`src/task_manager.rs:221-224, 300-302`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L221-L224)).
  - **Tokio Tasks**: **Yes**, `spawn` invokes `tokio::spawn(async move { ... })` and retains the `JoinHandle` on the entry until completion or abort ([`src/task_manager.rs:355-397`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L355-L397)).
  - **Expiry Semantics**: Two-phase opportunistic sweep (`sweep_expired`, [`src/task_manager.rs:508-537`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L508-L537)). Non-terminal tasks whose `ttl_ms` elapsed are aborted and transitioned to `TaskStatus::Failed` with `"task expired: TTL elapsed before completion"`. Terminal tasks are retained for one additional `ttl_ms` grace period window from `terminal_at` before eviction. No background timer runs; sweeps trigger on API calls. Tasks with `ttl_ms: None` have unlimited retention until [`TaskManager::shutdown`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L486-L493).
  - **Cancellation Semantics**: Cooperative ([`src/task_manager.rs:438-472`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L438-L472)). `cancel_task` sets `cancel_requested = true`, notifies via a `tokio::sync::watch` channel (`cancel_signal.send(true)`), and drops response senders in `pending_inputs` to unpark any awaiting `request_input` calls. It does not abort the task future or force terminal state directly; the running future chooses how to exit.

### 5. Cancellation
- **`notifications/cancelled` Handling**:
  - Client sends `notifications/cancelled` (`CancelledNotification`) with `CancelledNotificationParam { request_id, reason }` ([`src/model.rs:873-880`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L873-L880)).
  - Server message loop extracts `request_id` and looks up `local_ct_pool.remove(request_id)` ([`src/service.rs:1605-1619`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/service.rs#L1605-L1619)). If present, `ct.cancel()` cancels the [`CancellationToken`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/service.rs#L1204) in [`RequestContext`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/service.rs#L1200-L1223).
- **Interaction with Tasks**:
  - When `tools/call` materializes a task, [`CallToolResponse::Task(CreateTaskResult)`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/mrtr.rs#L112) is returned immediately to conclude the JSON-RPC request for `tools/call`.
  - The request ID is removed from `local_ct_pool` upon completion of the response. Any subsequent `notifications/cancelled` for that request ID is ignored.
  - Does `tasks/cancel` trigger `RequestContext::ct`? **No**. `tasks/cancel` is an independent JSON-RPC request targeting a `task_id` string. It invokes `ServerHandler::cancel_task`, triggering `TaskManager`'s internal watch channel ([`src/task_manager.rs:461`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L461)). It has no connection to `RequestContext::ct`.
  - Does `notifications/cancelled` trigger `TaskContext::cancelled()`? **No**. `TaskContext` listens strictly to `TaskManager`'s internal watch channel driven by `tasks/cancel`.

### 6. Result Delivery
- **How Final `CallToolResult` is Returned**:
  - `tasks/result` **does not exist** in `rmcp` 3.2.0.
  - Result delivery occurs by the client **polling `tasks/get`** until `info.task.status().is_terminal()` ([`tests/test_task.rs:154-167`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/tests/test_task.rs#L154-L167)).
  - When the task completes, [`TaskPayload::Completed`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L130-L133) inlines the result under the top-level `result` key of [`DetailedTask`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L195, 230).
  - High-level client note: `client.call_tool(...)` explicitly does not drive task polling. If the server materializes a task, `client.call_tool(...)` returns `Err(ServiceError::UnexpectedResponse)` ([`src/service/client.rs:1968`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/service/client.rs#L1968)). Callers desiring task handling must call `client.peer().call_tool_once(...)` and poll `tasks/get`.
- **Distinction Between Error Result vs. JSON-RPC Error**:
  - **Yes, explicitly distinguished**:
    - **Tool Error Result (`isError: true`)**: Tool execution returning `CallToolResult { is_error: Some(true), .. }` produces `Ok(CallToolResult)` in the task closure. This resolves as `TaskStatus::Completed` carrying [`TaskPayload::Completed { result }`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L28-L29, 130-133).
    - **JSON-RPC Error**: Returning `Err(TaskExit::Error(error))` (or task TTL expiration) settles the task as `TaskStatus::Failed` carrying [`TaskPayload::Failed { error }`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L30-L31, 134-138) where `error` is the JSON-RPC error object containing `code` and `message` ([`src/task_manager.rs:365-367`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L365-L367)).

### 7. Incomplete, TODO, or Experimental Elements
- **`notifications/tasks` cannot be routed via `subscriptions/listen`**: In [`src/service/server.rs:238-247`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/service/server.rs#L238-L247), attempting to send a `TaskStatusNotification` through a subscription session explicitly returns `Err(SubscriptionSendError::UnsupportedNotification("notifications/tasks"))` because `SubscriptionFilter` lacks a `taskIds` field. Clients are forced to poll `tasks/get`.
- **Stale reference to `TaskAugmentedRequestParamsMeta`**: Documented on deprecated `CreateMessageRequestParams` in [`src/model.rs:2855`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L2855), but the type does not exist in the crate.
- **Client polling helper omitted**: `client.call_tool(...)` returns `Err(ServiceError::UnexpectedResponse)` on task responses ([`src/service/client.rs:1968`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/service/client.rs#L1968)), requiring manual polling loops.
- **Task creation restricted to `tools/call`**: Only [`CallToolResponse`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/mrtr.rs#L105-L113) has a `Task(CreateTaskResult)` variant. Other request types (such as `prompts/get` in [`GetPromptResponse`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/mrtr.rs#L144-L151)) have no task variant.
- **No background sweeper in `TaskManager`**: Sweeps occur strictly opportunistically on incoming method calls ([`src/task_manager.rs:297-298, 508-537`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/task_manager.rs#L297-L298)). Tasks spawned with `ttl_ms: None` remain in memory indefinitely until server shutdown.

### 8. Wire Examples

#### (a) `tools/call` Request
Under SEP-2663, task execution is not requested via a dedicated `task` argument field. The client advertises task capability (at `initialize` or in request `_meta`), and the server decides whether to return `CreateTaskResult`.
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "generate_report",
    "_meta": {
      "io.modelcontextprotocol/clientCapabilities": {
        "extensions": {
          "io.modelcontextprotocol/tasks": {}
        }
      }
    },
    "arguments": {
      "project": "core"
    }
  }
}
```
*Field requirements*:
- `jsonrpc`: `"2.0"` (required)
- `id`: integer or string (required)
- `method`: `"tools/call"` (required)
- `params.name`: string (required)
- `params._meta`: object (optional)
- `params.arguments`: object (optional)
- `params.inputResponses`: object (optional; used only in MRTR retry)
- `params.requestState`: string (optional; used only in MRTR retry)

#### (b) `CreateTaskResult` Response
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "resultType": "task",
    "taskId": "7c9e6679-7425-40de-944b-e07fc1f90ae7",
    "status": "working",
    "statusMessage": "Processing data",
    "createdAt": "2026-09-05T18:45:00Z",
    "lastUpdatedAt": "2026-09-05T18:45:00Z",
    "ttlMs": 300000,
    "pollIntervalMs": 1000
  }
}
```
*Field requirements*:
- `result.resultType`: `"task"` (required)
- `result.taskId`: string (required)
- `result.status`: `"working"` | `"input_required"` | `"completed"` | `"failed"` | `"cancelled"` (required)
- `result.createdAt`: ISO 8601 string (required)
- `result.lastUpdatedAt`: ISO 8601 string (required)
- `result.ttlMs`: integer or `null` (required on wire; `null` denotes unlimited retention)
- `result.statusMessage`: string (optional)
- `result.pollIntervalMs`: integer (optional)
- `result._meta`: object (optional)

#### (c) `tasks/get` Request and Response
**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tasks/get",
  "params": {
    "taskId": "7c9e6679-7425-40de-944b-e07fc1f90ae7"
  }
}
```
*Field requirements*:
- `params.taskId`: string (required)
- `params._meta`: object (optional)

**Response (Terminal Completed)**:
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "resultType": "complete",
    "taskId": "7c9e6679-7425-40de-944b-e07fc1f90ae7",
    "status": "completed",
    "createdAt": "2026-09-05T18:45:00Z",
    "lastUpdatedAt": "2026-09-05T18:45:10Z",
    "ttlMs": 300000,
    "pollIntervalMs": 1000,
    "result": {
      "content": [
        {
          "type": "text",
          "text": "Report completed successfully"
        }
      ],
      "isError": false
    }
  }
}
```
*Field requirements*:
- `result.resultType`: `"complete"` (required / default `"complete"`)
- `result.taskId`: string (required)
- `result.status`: string (required)
- `result.createdAt`: string (required)
- `result.lastUpdatedAt`: string (required)
- `result.ttlMs`: integer or `null` (required on wire)
- `result.statusMessage`: string (optional)
- `result.pollIntervalMs`: integer (optional)
- `result.result`: object (required when status is `"completed"`)
- `result.error`: object (required when status is `"failed"`)
- `result.inputRequests`: object (required when status is `"input_required"`)

#### (d) `tasks/result` Request and Response
**NOT FOUND in rmcp 3.2.0**.
SEP-2663 in `rmcp` 3.2.0 does not define a `tasks/result` method. Task results are retrieved by polling `tasks/get` until reaching `status: "completed"` where `result` is inlined.

*(For intermediate inputs, `tasks/update` is used)*:
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tasks/update",
  "params": {
    "taskId": "7c9e6679-7425-40de-944b-e07fc1f90ae7",
    "inputResponses": {
      "req-1": { "confirmed": true }
    }
  }
}
```

#### (e) `tasks/cancel` Request and Response
**Request**:
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "tasks/cancel",
  "params": {
    "taskId": "7c9e6679-7425-40de-944b-e07fc1f90ae7"
  }
}
```
*Field requirements*:
- `params.taskId`: string (required)
- `params._meta`: object (optional)

**Response**:
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "result": {
    "resultType": "complete"
  }
}
```
*Field requirements*:
- `result.resultType`: `"complete"` (required; strictly verified by `TaskAckResult` deserializer with `deny_unknown_fields`)
- `result._meta`: object (optional)

---

## Contradictions and Uncertainties

1. **Protocol Version Gating Docstring vs. Implementation**:
   - In [`src/model.rs:4580-4584`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4580-L4584), the docstring for `strip_result_type_for_legacy_peer` asserts:
     *"results whose discriminator carries meaning (`"input_required"`, `"task"`) are already gated to `2026-07-28`+ sessions"*.
   - In the actual server dispatch code at [`src/handler/server.rs:246-259`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/handler/server.rs#L246-L259), **only** `InputRequiredResult` is gated on `sep_2322_supported` (`2026-07-28`+). `CreateTaskResult` is not gated on protocol version, allowing a server to return `CreateTaskResult` to legacy sessions (such as `2025-11-25`) as long as the client declared the tasks extension capability.
2. **Missing `TaskAugmentedRequestParamsMeta` Trait**:
   - The docstring for `CreateMessageRequestParams` ([`src/model.rs:2855`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L2855)) references `TaskAugmentedRequestParamsMeta`, but no such trait or struct exists anywhere in the codebase.
3. **Notification Method Name Discrepancy**:
   - While some external documentation references `notifications/tasks/status`, `rmcp` 3.2.0 names the notification method `notifications/tasks` ([`src/model.rs:4334`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4334)).

---

## Discriminating Tests to Prove Wire Compatibility

1. An incoming `tools/call` JSON request carrying an extraneous `task: {"ttl": 60000}` field deserializes without error into [`CallToolRequestParams`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model.rs#L4054-L4071) and is executed synchronously when task capability is not declared.
2. Deserialization of [`CreateTaskResult`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L278-L290) fails with a custom deserializer error when `resultType` is absent or set to any value other than `"task"`.
3. A [`Task`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L48-L70) with `ttl_ms: None` serializes with `"ttlMs": null` on the wire rather than omitting the field.
4. Deserialization of [`DetailedTask`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L163-L183) fails if `status == "input_required"` without `inputRequests`, `status == "completed"` without `result`, or `status == "failed"` without `error`.
5. Dispatch of `tasks/get` or `tasks/cancel` when the peer did not advertise `io.modelcontextprotocol/tasks` capability yields JSON-RPC error `-32021` (`Missing Required Client Capability`) with capability data.
6. A `tasks/get` request naming an unknown or expired task ID returns JSON-RPC error `-32602` (`Invalid params`) rather than `-32601` or `-32603`.
7. Deserialization of [`TaskAckResult`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/task.rs#L377-L415) strictly enforces `resultType == "complete"` and rejects any unexpected payload fields via `deny_unknown_fields`.
8. A tool call returning [`CallToolResult`](file:///Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.2.0/src/model/tool.rs) with `isError: true` inside a task resolves with task status `"completed"` and inlined `result.isError: true`, rather than status `"failed"`.

---

## Limitations of this Review
- **Execution**: Per instructions, no `cargo test`, compilation, or network operations were executed. All findings are derived strictly from static analysis of the verbatim `rmcp` 3.2.0 package source files and test fixtures.
- **Source Scope**: Review was restricted strictly to the crate source files located in `rmcp-3.2.0` (matching SHA-256 `0ad47897f5429df21ae73d743563b2ffd03d58453a3b0ea4364f97e3d3b8e362`). No external Git repositories, upstream pull requests, or uncached crate versions were accessed.
