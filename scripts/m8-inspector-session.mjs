#!/usr/bin/env node
// Stock Inspector 2.5.0 qualification for the 0.8.0 wire/client matrix (M8-04).
//
// Python owns every decision: it builds the attempt-local export-only bridge,
// the closed server argv, the frozen contract manifest and the whole
// Docker-free negative call plan (arguments, the declared error vocabulary
// each row must stay inside). This file only drives the stock client and
// reports the oracles it observed, so no expectation can drift between the
// harness and the session.
import process from "node:process";
import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import { pathToFileURL } from "node:url";

const [bridgePath, serverArgvJson, planJson] = process.argv.slice(2);
if (!bridgePath || !serverArgvJson || !planJson) {
  throw new Error("usage: m8-inspector-session.mjs BRIDGE SERVER_ARGV_JSON PLAN_JSON");
}
const serverArgv = JSON.parse(serverArgvJson);
if (!Array.isArray(serverArgv) || serverArgv.length === 0) throw new Error("server argv must be non-empty");
const plan = JSON.parse(planJson);
if (!Array.isArray(plan.expected_tools) || plan.expected_tools.length === 0) throw new Error("expected inventory missing");
if (!Array.isArray(plan.negative_rows)) throw new Error("negative call plan missing");
if (!Array.isArray(plan.generic_negatives)) throw new Error("generic negative plan missing");
const { InspectorClient, createTransportNode } = await import(pathToFileURL(bridgePath).href);

const RUNTIME = "runtime";
// The Docker-free negative call plan only holds against a host with no
// calibrated runtime; a `runtime` session's plan carries neither list
// (Python never fills them for this mode), and this session never executes
// them -- it composes its own positive oracle below instead.
if (plan.mode !== RUNTIME && plan.negative_rows.length === 0) throw new Error("negative call plan missing");

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort((left, right) => (left < right ? -1 : left > right ? 1 : 0)).map((name) => [name, stable(value[name])]));
  }
  return value;
}
function canonicalHash(value) {
  const sorted = stable(value === undefined ? null : value);
  const bytes = Buffer.from(JSON.stringify(sorted), "utf8");
  return createHash("sha256").update(bytes).digest("hex");
}
function toolByName(tools, name) {
  const tool = tools.find((candidate) => candidate.name === name);
  if (!tool) throw new Error(`tool absent: ${name}`);
  return tool;
}

function assembleArguments(row, ref) {
  const arguments_ = JSON.parse(JSON.stringify(row.arguments ?? {}));
  for (const field of row.project_ref_fields) {
    arguments_[field] = ref.projectRef;
  }
  if (row.fingerprint_target === "top") {
    arguments_.expected_project_fingerprint = ref.fingerprint;
  } else if (row.fingerprint_target === "action") {
    arguments_.action = { ...(arguments_.action ?? {}), expected_project_fingerprint: ref.fingerprint };
  }
  return arguments_;
}

async function callNegative(client, tools, row, ref) {
  const args = assembleArguments(row, ref);
  const { result } = await client.callTool(toolByName(tools, row.tool), args);
  const structured = result?.structuredContent;
  return {
    tool: row.tool,
    status: structured?.status ?? null,
    error_code: structured?.error_code ?? null,
    is_error: result?.isError === true,
  };
}

async function callGeneric(client, tools, row, ref) {
  if (row.kind === "unknown_project_ref") {
    const { result } = await client.callTool(toolByName(tools, row.tool), row.arguments);
    const structured = result?.structuredContent;
    return {
      kind: row.kind, status: structured?.status ?? null,
      error_code: structured?.error_code ?? null, is_error: result?.isError === true,
    };
  }
  // unknown_tool / invalid_args / unknown_fields: each is refused at the
  // protocol boundary (an unknown tool name, a schema-invalid argument or an
  // additional field outside a closed object), never as a structured result.
  // The JSON-RPC `error.code` the SDK surfaces on the thrown McpError is the
  // actual wire code the server answered with -- MethodNotFound (-32601) or
  // InvalidParams (-32602) -- never a stand-in for "some exception happened".
  try {
    if (row.kind === "unknown_tool") {
      await client.callTool({ name: row.tool }, row.arguments);
    } else {
      await client.callTool(toolByName(tools, row.tool), row.arguments);
    }
    return { kind: row.kind, protocol_error: false, rpc_code: null };
  } catch (error) {
    const rpc_code = typeof error?.code === "number" ? error.code : null;
    return { kind: row.kind, protocol_error: true, rpc_code };
  }
}

const serverConfig = {
  type: "stdio", command: serverArgv[0], args: serverArgv.slice(1),
  env: Object.fromEntries(["HOME", "PATH", "TMPDIR", "USER", "LOGNAME", "SHELL"]
    .filter((name) => process.env[name] !== undefined).map((name) => [name, process.env[name]])),
};
const client = new InspectorClient(serverConfig, {
  environment: { transport: createTransportNode },
  clientIdentity: { name: "mcp-inspector", version: "2.5.0" },
  sample: false, elicit: false, progress: false, roots: [],
  advertisedExtensions: {},
  versionNegotiation: { mode: { pin: "2026-07-28" } },
  timeout: plan.request_timeout_ms,
  serverSettings: {
    protocolEra: "modern", connectionTimeout: 15_000,
    requestTimeout: plan.request_timeout_ms,
  },
});

try {
  await client.connect();
  if (client.getProtocolEra() !== "modern") throw new Error("Inspector did not negotiate modern MCP");
  const { tools } = await client.listAllTools({ cacheMode: "refresh" });
  const names = tools.map((tool) => tool.name);
  if (names.length !== plan.expected_tools.length || JSON.stringify(names) !== JSON.stringify(plan.expected_tools)) {
    throw new Error("Inspector ordered inventory mismatch");
  }
  const resources = await client.listAllResources({ cacheMode: "refresh" });
  const resourcesList = (resources?.resources ?? []).map((resource) => resource.uri);

  // The contract-equality oracle: recompute the same canonical hashes
  // `scripts/contract-freeze.py` uses, from this live `tools/list` response.
  const contract = {};
  for (const tool of tools) {
    contract[tool.name] = {
      annotations: tool.annotations ?? null,
      input_schema_sha256: canonicalHash(tool.inputSchema),
      output_schema_sha256: canonicalHash(tool.outputSchema),
      description_sha256: canonicalHash(tool.description ?? null),
    };
  }

  const opened = await client.callTool(toolByName(tools, "rust.project.open"), { path: plan.fixture });
  if (opened.result?.isError === true) throw new Error("Inspector could not open the fixture project");
  const openedData = opened.result?.structuredContent?.data;
  if (typeof openedData?.project_ref !== "string" || typeof openedData?.fingerprint !== "string") {
    throw new Error("Inspector open did not return a project_ref/fingerprint");
  }
  const ref = { projectRef: openedData.project_ref, fingerprint: openedData.fingerprint };

  const negativeRows = [];
  const genericNegatives = [];
  if (plan.mode !== RUNTIME) {
    for (const row of plan.negative_rows) {
      negativeRows.push(await callNegative(client, tools, row, ref));
    }
    for (const row of plan.generic_negatives) {
      genericNegatives.push(await callGeneric(client, tools, row, ref));
    }
  }

  const outcome = {
    version: "2.5.0", protocol_era: "modern", mode: plan.mode, tool_count: names.length,
    discovery: true, resources_list: resourcesList,
    contract, negative_rows: negativeRows, generic_negatives: genericNegatives,
  };

  if (plan.mode === RUNTIME) {
    const checkResult = await client.callTool(toolByName(tools, "rust.check"), { project_ref: ref.projectRef });
    const checkStructured = checkResult.result?.structuredContent;
    outcome.runtime_check_status = checkStructured?.status ?? null;
    if (checkStructured?.status !== "passed") {
      throw new Error(`Inspector runtime rust.check did not pass: ${checkStructured?.status}`);
    }
    const findArtifactUri = (value) => {
      if (typeof value === "string" && value.startsWith("rust-artifact://")) return value;
      if (Array.isArray(value)) {
        for (const item of value) {
          const found = findArtifactUri(item);
          if (found) return found;
        }
      } else if (value !== null && typeof value === "object") {
        for (const item of Object.values(value)) {
          const found = findArtifactUri(item);
          if (found) return found;
        }
      }
      return null;
    };
    const artifactUri = findArtifactUri(checkStructured);
    if (!artifactUri) throw new Error("runtime rust.check published no rust-artifact:// URI");
    const resourceRead = await client.readResource(artifactUri);
    const readContents = resourceRead?.result?.contents;
    outcome.resource_read_ok = Array.isArray(readContents) && readContents.length > 0
      && readContents.every((item) => typeof item?.uri === "string" && (typeof item?.blob === "string" || typeof item?.text === "string"));
    if (!outcome.resource_read_ok) throw new Error("Inspector could not read the published artifact");

    // G4: one call cancelled mid-flight is rejected through the SDK's own
    // non-Tasks cancellation. `cancelToolCall()` aborts the call's own
    // internal AbortController, which the SDK's `tools/call` request path
    // carries to the server as `notifications/cancelled` on stdio (the
    // per-request-stream abort is only a modern Streamable HTTP mechanism).
    // The short delay before cancelling matters: called synchronously in the
    // same tick as `callTool()`, the abort can fire before the request has
    // even reached the transport's `send()`, so nothing goes out over the
    // wire (no `tools/call`, no cancellation notification) and there is
    // nothing for the proxy to observe. Waiting here makes sure the request
    // is genuinely in flight, so both the cancellation notification and the
    // retry's success are real network evidence, not a race against the
    // request never having been sent. A fresh, uncancelled retry then
    // succeeding is the evidence that the server's cleanup joined cleanly.
    const pending = client.callTool(toolByName(tools, "rust.check"), { project_ref: ref.projectRef });
    await new Promise((resolve) => setTimeout(resolve, 300));
    const cancelled = client.cancelToolCall();
    if (!cancelled) throw new Error("Inspector had no in-flight call to cancel");
    let rejected = false;
    try {
      await pending;
    } catch {
      rejected = true;
    }
    if (!rejected) throw new Error("Inspector's cancelled call resolved instead of rejecting");
    // The local rejection above only reflects the SDK's own bookkeeping --
    // it settles as soon as the abort fires, well before the server has
    // necessarily finished tearing down the cancelled call's container. A
    // retry sent immediately can race that teardown and land on the
    // gateway's single-flight `SandboxDenied`/`Busy` refusal, which is not
    // the same thing as the server failing to clean up -- it is the server
    // still cleaning up. Poll briefly so the oracle asks "did cleanup join
    // within a reasonable bound", not "was cleanup already done the instant
    // the local promise rejected".
    let retried;
    const retryDeadline = Date.now() + 10_000;
    do {
      retried = await client.callTool(toolByName(tools, "rust.check"), { project_ref: ref.projectRef });
      outcome.cancel_ok = retried.result?.isError !== true
        && retried.result?.structuredContent?.status === "passed";
      if (outcome.cancel_ok) break;
      await new Promise((resolve) => setTimeout(resolve, 500));
    } while (Date.now() < retryDeadline);
    if (!outcome.cancel_ok) throw new Error("Inspector's post-cancel retry did not observe joined cleanup");

    // G5: a client that vanishes mid-call (stdin EOF, no orderly `shutdown`
    // and no `notifications/cancelled`) must still let the server tear its
    // worker down. Launch one more `rust.check`, then close this session's
    // transport without waiting on either the call or the close to settle --
    // exactly what an abruptly-vanishing client looks like on the wire. The
    // proof that the server actually reaped its worker is a brand-new
    // session, against the very same server argv (state-root included),
    // succeeding right after: `tools/list` only needs the server to be
    // accepting a fresh connection, never the torn-down one.
    const eofPending = client.callTool(toolByName(tools, "rust.check"), { project_ref: ref.projectRef });
    eofPending.catch(() => {});
    outcome.eof_prior_pid = client.baseTransport?.pid ?? null;
    client.baseTransport?.close()?.catch(() => {});

    const eofClient = new InspectorClient(serverConfig, {
      environment: { transport: createTransportNode },
      clientIdentity: { name: "mcp-inspector", version: "2.5.0" },
      sample: false, elicit: false, progress: false, roots: [],
      advertisedExtensions: {},
      versionNegotiation: { mode: { pin: "2026-07-28" } },
      timeout: plan.request_timeout_ms,
      serverSettings: {
        protocolEra: "modern", connectionTimeout: 15_000,
        requestTimeout: plan.request_timeout_ms,
      },
    });
    try {
      await eofClient.connect();
      const { tools: eofTools } = await eofClient.listAllTools({ cacheMode: "refresh" });
      outcome.eof_new_session_ok = eofTools.length === plan.expected_tools.length;
    } finally {
      await eofClient.disconnect(5_000);
    }
    if (!outcome.eof_new_session_ok) {
      throw new Error("a fresh session after the client's mid-call EOF did not see the tool inventory");
    }
  }

  process.stdout.write(`${JSON.stringify(outcome)}\n`);
} finally {
  await client.disconnect(5_000);
}
