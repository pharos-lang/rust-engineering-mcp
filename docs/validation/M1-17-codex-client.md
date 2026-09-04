# M1-17 stock Codex client qualification

Status: **direct client APIs passed; model-driven tool use not qualified**.
This is supplemental evidence. MCP Inspector remains the required external-client
qualification for the M1 Definition of Done.

The tested client was the stock Codex CLI/app-server 0.153.0 binary, SHA-256
`a29d9e86eef88cbbd69f97ce8c590b1d0a287c8f77424f5eef226b883d7eaa22`.
The product was the source-accepted local candidate binary, SHA-256
`7a99038be57429e1db32c91d01772e7efd104691828253f45ed3bbb0e9330417`.
Configuration was process-local: one required `rust_engineering` server, the exact
13-tool allowlist, `default_tools_approval_mode=approve`, host servers `node_repl`
and `youtrack` explicitly disabled, no global config mutation, no workspace roots,
and shell, apps, plugins, browser, code mode, collaboration and other unrelated
capabilities disabled.

`approve` was a qualification-harness setting after an independently approved
exact plan and allowlist; it is not a recommended client posture for these
code-executing tools. Historical receipt maps named `disabled_features` and
`disabled_host_servers` contain the effective `enabled` values, so `false` means
disabled. The [semantics sidecar](m1-17-codex-client/effective-config-semantics.json)
records this and the current controller uses unambiguous field names.

## Direct app-server preflight

Two source- and plan-bound preflights passed. Plan v1
(`08319ae703db61675b60b1f9fe923dd67ec7f47cf9246fdf3d52dc5cc705ceff`)
is recorded in [attempt 5](m1-17-codex-client/preflight-attempt-5.json). Plan v2
(`079f79f722b82d15821c78d88e7f2b21374ed0f958253bf07e65ca170bee6ac7`)
is recorded in [attempt 6](m1-17-codex-client/preflight-attempt-6.json).

Each preflight used native app-server methods to:

1. read and fail closed on the effective configuration;
2. start an ephemeral OpenAI `gpt-5.6-sol` medium thread with no instruction files
   or runtime workspace roots;
3. discover exactly 13 active product tools while both host servers had empty tool
   maps;
4. call `rust.project.open`, `rust.catalog.status`, `rust.project.inspect` and
   `rust.check` through `mcpServer/tool/call`;
5. require `status=passed` and the expected catalog fingerprint;
6. read the emitted artifact through `mcpServer/resource/read`;
7. verify no product Docker object remained, every observed process ended naturally,
   and the private state root was empty after exact bounded cleanup.

The four historical `tools_sha256` values were insertion-order encodings of JSON
maps, not contract digests. Two new status-only sessions returned deep-equal full
tool maps: their wire-order hashes differ, while their canonical JSON SHA-256 is
the same (`7c83911d4a75f693bb661e4b93535af32c2fb5c90f0bdf781821cf0be3b57136`).
The [canonicalization receipt](m1-17-codex-client/tool-inventory-canonicalization.json)
retains all thirteen per-tool canonical hashes. No model turn or product tool call
was used for that check.

## Candidate-bound repair and missing runtime mechanics

A fresh direct stock-client session on the exact candidate binary opened a temporary
fixture, received a structured failed `rust.check` with E0502, observed an external
source edit, reopened the changed project and received a passing check. A second
session configured a deliberately nonexistent Docker executable and received a
structured `SANDBOX_DENIED`/blocked result. The exact candidate CLI independently
reported `status=unavailable`, `reason=InvalidConfiguration`, exit 1 and empty
stderr. Both transports joined naturally, the
private state root was empty and the temporary fixture and Docker objects were
absent afterward. See the [receipt](m1-17-codex-client/repair-missing-receipt.json).

The edit actor was the principal qualification harness. This proves client-observed
repair and missing-runtime mechanics; M1-16 separately proves bounded model-authored
patches. It is not described as a stock-Codex model repair loop.

The first four preflight receipts are deliberately retained as failed harness
iterations. They found an unsupported config field, inherited host servers, the
0.153.0 status-map shape, and the need to allow a short natural child-process settle
plus remove exactly four owner-only generated sandbox files. None of the first three
called a product tool. Attempt 4 completed the same direct calls but failed its
over-strict immediate cleanup check; its postcheck records the exact files removed.

## Bounded model turns

The first prompt requested one `rust.catalog.status` call. The turn completed with
zero tool calls and returned `status: unavailable`; the receipt is
[model attempt 1](m1-17-codex-client/model-attempt-1.json).

The second plan made Codex's deferred-tool behavior explicit and requested one
internal tool search followed by one product call. Raw app-server events show that
the model instead emitted one `custom_tool_call` named `exec` to inspect
`ALL_TOOLS`. The disabled code-mode host returned `code-mode host is disabled`.
No search item and no product `mcpToolCall` occurred, so the verifier rejected the
turn. See [model attempt 2](m1-17-codex-client/model-attempt-2.json) and its
[sanitized event sequence](m1-17-codex-client/model-attempt-2.events.sanitized.jsonl).

Code mode was not enabled for another attempt because that would grant new execution
authority solely to force the qualification. The final controller now rejects such
unexpected raw custom/function calls immediately; its 11 discriminating tests pass.
Therefore these runs prove stock Codex's direct MCP client methods and Resource read,
but they do not prove model-driven use in this closed profile.

The candidate binary's 238 selected inputs also match `d024c7c` in the
[source-equivalence receipt](../release/candidate/m1-17-source-equivalence.json).
The [preservation receipt](m1-17-codex-client/preservation-receipt.json) binds the
original ignored event streams and every retained artifact. Repository copies of
the event streams remove raw system/developer messages and account rate-limit
payloads while keeping ordering, IDs, tool attempts, output, completion and usage.
The bounded secret scan is empty. [Official source references](m1-17-codex-client/official-sources.md)
record the current MCP and app-server contracts consulted.
