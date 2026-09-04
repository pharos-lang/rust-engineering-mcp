# Stock Codex 0.153.0 qualification harness

The closed controller exercises Codex app-server through stdio with one required
Rust Engineering MCP server, an exact 13-tool allowlist and all discovered host
servers disabled process-locally. Preflight attempt 5 (plan v1) and attempt 6
(plan v2) passed direct tool and Resource calls with identity, Docker and cleanup
checks. Two bounded model turns failed closed without a product MCP call. The first
made no tool call. The second attempted Codex's internal `exec` discovery path and
received `code-mode host is disabled`; code mode was not enabled to force a pass.

`controller-executed-attempt-2.py` is the exact controller used by the second model
turn. `controller.py` adds the postmortem rejection of unexpected raw custom or
function tool calls. The repository retains sanitized event projections and hashes
the ignored raw streams.

Follow-up qualification adds two status-only inventory sessions whose full maps are
deep-equal under canonical JSON, and a direct client session that observes E0502,
an external fixture edit, a passing re-check and a structured missing-runtime denial.
These are client mechanics with zero model turns; they do not convert the failed
stock-model attempts into model-driven success. Historical insertion-order hashes
and inverted field names are explained by repo-visible sidecars.
