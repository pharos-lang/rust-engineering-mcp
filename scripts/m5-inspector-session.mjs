#!/usr/bin/env node
// Stock Inspector 2.5.0 qualification for the four M5 tools.
//
// Python owns every decision: it builds the attempt-local export-only bridge,
// the closed server argv and the whole call plan (arguments, the exact declared
// status/error_code each call must produce, and the observation facts a real
// measurement must carry).  This file only drives the stock client and reports
// the oracles it observed, so no expectation can drift between the harness and
// the session.
//
// The same driver serves both modes.  In the Docker-free mode every planned
// call is a declared refusal the server produces before a container exists; in
// the runtime mode two calls are real measurements whose published artifacts
// are read back as Resources.
import process from "node:process";
import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import { pathToFileURL } from "node:url";

const [bridgePath, serverArgvJson, planJson] = process.argv.slice(2);
if (!bridgePath || !serverArgvJson || !planJson) {
  throw new Error("usage: m5-inspector-session.mjs BRIDGE SERVER_ARGV_JSON PLAN_JSON");
}
const serverArgv = JSON.parse(serverArgvJson);
if (!Array.isArray(serverArgv) || serverArgv.length === 0) throw new Error("server argv must be non-empty");
const plan = JSON.parse(planJson);
if (!Array.isArray(plan.expected_tools) || plan.expected_tools.length === 0) throw new Error("expected inventory missing");
if (!Array.isArray(plan.calls) || plan.calls.length === 0) throw new Error("call plan missing");
if (plan.projects === null || typeof plan.projects !== "object") throw new Error("project plan missing");
const { InspectorClient, createTransportNode } = await import(pathToFileURL(bridgePath).href);

const TASKS = "io.modelcontextprotocol/tasks";
const ARTIFACT_SCHEME = "rust-quality-artifact://";

// Recursively key-sorted encoding, so a row's digest depends on the value and
// not on property order.  Python's `json.dumps(..., sort_keys=True,
// separators=(",", ":"), ensure_ascii=False)` produces the same bytes.
function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((name) => [name, stable(value[name])]));
  }
  return value;
}
function measure(value) {
  const sorted = stable(value);
  const bytes = Buffer.from(JSON.stringify(sorted === undefined ? null : sorted), "utf8");
  return { bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
}
function valuesFor(value, key, found = []) {
  if (Array.isArray(value)) for (const child of value) valuesFor(child, key, found);
  else if (value !== null && typeof value === "object") {
    for (const [name, child] of Object.entries(value)) {
      if (name === key) found.push(child);
      valuesFor(child, key, found);
    }
  }
  return found;
}
function uniqueString(value, key) {
  const values = [...new Set(valuesFor(value, key).filter((item) => typeof item === "string"))];
  if (values.length !== 1) throw new Error(`${key} was absent or ambiguous`);
  return values[0];
}
function toolByName(tools, name) {
  const tool = tools.find((candidate) => candidate.name === name);
  if (!tool) throw new Error(`tool absent: ${name}`);
  return tool;
}
function isInteger(value) {
  return typeof value === "number" && Number.isInteger(value);
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
  advertisedExtensions: { [TASKS]: true },
  versionNegotiation: { mode: { pin: "2026-07-28" } },
  serverSettings: {
    protocolEra: "modern", connectionTimeout: 15_000,
    requestTimeout: plan.request_timeout_ms,
  },
});

try {
  await client.connect();
  if (client.getProtocolEra() !== "modern") throw new Error("Inspector did not negotiate modern MCP");
  if (client.getCapabilities()?.extensions?.[TASKS] === undefined) throw new Error("server omitted Tasks");
  const { tools } = await client.listAllTools({ cacheMode: "refresh" });
  const names = tools.map((tool) => tool.name);
  if (names.length !== plan.expected_tools.length || JSON.stringify(names) !== JSON.stringify(plan.expected_tools)) {
    throw new Error("Inspector ordered inventory mismatch");
  }

  // One authority per fixture root, resolved once and reused by every call.
  const refs = {};
  for (const [name, path] of Object.entries(plan.projects)) {
    if (typeof path !== "string" || path.length === 0) throw new Error(`fixture root missing: ${name}`);
    const opened = await client.callTool(toolByName(tools, "rust.project.open"), { path });
    if (opened.result?.isError === true) throw new Error(`Inspector could not open ${name}`);
    refs[name] = uniqueString(opened.result, "project_ref");
  }

  const rows = [];
  const facts = {};
  const refusalUris = new Set();
  for (const row of plan.calls) {
    const reference = row.project === null ? plan.unknown_project_ref : refs[row.project];
    if (typeof reference !== "string") throw new Error(`project authority missing for ${row.tool}`);
    const args = { project_ref: reference, ...row.arguments };
    const request = measure(args);
    let result;
    try {
      result = (await client.callTool(toolByName(tools, row.tool), args)).result;
    } catch (error) {
      // Every planned call is a declared result; a masked RPC error means the
      // server refused at the protocol layer, which this gate does not accept.
      throw new Error(`Inspector ${row.tool} ${row.shape} produced a protocol error: ${error?.code ?? "unknown"}`);
    }
    const structured = result?.structuredContent;
    const label = `${row.tool} ${row.mode} ${row.shape}`;
    if ((result?.isError === true) !== row.expect_is_error) {
      throw new Error(`Inspector ${label} isError was not ${row.expect_is_error}`);
    }
    if (structured?.status !== row.expect_status) {
      throw new Error(`Inspector ${label} status ${structured?.status} != ${row.expect_status}`);
    }
    if ((structured?.error_code ?? null) !== (row.expect_error_code ?? null)) {
      throw new Error(`Inspector ${label} error_code ${structured?.error_code} != ${row.expect_error_code}`);
    }

    // Runtime rows carry a real observation; the plan says exactly which facts
    // must hold, so a measurement that silently degraded fails here.
    let published = 0;
    let read = 0;
    if (row.mode === "runtime") {
      const observation = structured?.data?.observation;
      if (observation === null || typeof observation !== "object") {
        throw new Error(`Inspector ${label} published no observation`);
      }
      for (const [key, value] of Object.entries(row.expect_observation ?? {})) {
        if (observation[key] !== value) {
          throw new Error(`Inspector ${label} observation.${key} ${observation[key]} != ${value}`);
        }
      }
      for (const key of row.expect_positive_fields ?? []) {
        if (!isInteger(observation[key]) || observation[key] <= 0) {
          throw new Error(`Inspector ${label} observation.${key} is not a positive count`);
        }
      }
      for (const key of row.expect_zero_fields ?? []) {
        if (observation[key] !== 0) throw new Error(`Inspector ${label} observation.${key} is not zero`);
      }
      if (row.expect_measured === true) {
        const measured = observation.measured;
        if (measured === null || typeof measured !== "object"
            || !isInteger(measured.size_bytes) || measured.size_bytes <= 0
            || !/^sha256:[0-9a-f]{64}$/.test(measured.sha256 ?? "")
            || measured.analysis_build_symbols_forced !== true) {
          throw new Error(`Inspector ${label} published no measured binary`);
        }
      }
      const artifacts = structured?.data?.artifacts;
      if (!Array.isArray(artifacts) || artifacts.length < row.expect_min_artifacts) {
        throw new Error(`Inspector ${label} published ${artifacts?.length ?? 0} artifacts, expected at least ${row.expect_min_artifacts}`);
      }
      published = artifacts.length;
      for (const artifact of artifacts) {
        if (typeof artifact.uri !== "string" || !artifact.uri.startsWith(ARTIFACT_SCHEME)
            || !/^[0-9a-f]{64}$/.test(artifact.sha256 ?? "")
            || !isInteger(artifact.size_bytes) || artifact.size_bytes <= 0) {
          throw new Error(`Inspector ${label} published an invalid artifact descriptor`);
        }
        const resource = await client.readResource(artifact.uri);
        const contents = resource.result?.contents;
        if (!Array.isArray(contents) || contents.length === 0) {
          throw new Error(`Inspector ${label} could not read its published Resource`);
        }
        read += 1;
      }
      if (read !== published) throw new Error(`Inspector ${label} did not read every published artifact`);
      facts[row.tool] = {
        artifacts_published: published, artifacts_read: read,
        ...Object.fromEntries((row.report_fields ?? [])
          .filter((key) => observation[key] !== undefined)
          .map((key) => [key, observation[key]])),
      };
    } else {
      for (const uri of valuesFor(result, "uri")) {
        if (typeof uri === "string" && uri.startsWith(ARTIFACT_SCHEME)) refusalUris.add(uri);
      }
    }

    const response = measure(result);
    rows.push({
      client: "inspector", tool: row.tool, shape: row.shape, mode: row.mode,
      status: structured.status, error_code: structured.error_code ?? null,
      is_error: result?.isError === true,
      artifacts_published: published, artifacts_read: read,
      request_bytes: request.bytes, request_sha256: request.sha256,
      response_bytes: response.bytes, response_sha256: response.sha256,
    });
  }

  // A declared refusal publishes no evidence, so in the Docker-free mode the
  // Resource oracle is the missing-artifact refusal, which is still a real
  // resources/read carried by the stock client.
  let resourcesRead = rows.reduce((total, row) => total + row.artifacts_read, 0);
  for (const uri of refusalUris) {
    const resource = await client.readResource(uri);
    if (!Array.isArray(resource.result?.contents) || resource.result.contents.length === 0) {
      throw new Error("Inspector Resource read failed");
    }
    resourcesRead += 1;
  }
  let missingRefused = false;
  if (resourcesRead === 0) {
    try {
      const resource = await client.readResource(plan.missing_resource_uri);
      missingRefused = resource?.result?.isError === true || resource?.error !== undefined;
    } catch {
      missingRefused = true;
    }
    if (!missingRefused) throw new Error("Inspector missing-Resource oracle was not observed");
  }

  process.stdout.write(`${JSON.stringify({
    version: "2.5.0", protocol_era: "modern", mode: plan.mode, tool_count: names.length,
    m5_tool_count: plan.m5_tools.length, discovery: true,
    positive_calls: rows.filter((row) => row.shape === "positive").length,
    negative_calls: rows.filter((row) => row.shape === "negative").length,
    resource: resourcesRead > 0 || missingRefused,
    artifact_resources_read: resourcesRead,
    missing_resource_refused: missingRefused,
    runtime_facts: facts,
    tasks_declared: true, tasks_advertised: true, calls: rows,
  })}\n`);
} finally {
  await client.disconnect(5_000);
}
