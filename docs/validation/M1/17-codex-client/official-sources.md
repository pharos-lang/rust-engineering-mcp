# Stock Codex source references

Checked 2026-09-04 against Codex CLI 0.153.0.

- https://learn.chatgpt.com/docs/extend/mcp?surface=cli — STDIO MCP support, required/enabled servers, exact enabled-tools allowlist, approval modes and timeouts.
- https://learn.chatgpt.com/docs/app-server — `config/read`, `mcpServerStatus/list`, `mcpServer/tool/call`, `mcpServer/resource/read`, thread/turn methods and raw-event opt-in.
- Locally generated 0.153.0 experimental JSON Schema was used to verify `experimentalRawEvents`, `mcpToolCall`, `tool_search_call` and `tool_search_output` shapes; the generated bundle remains an ignored target artifact and is not product evidence.
