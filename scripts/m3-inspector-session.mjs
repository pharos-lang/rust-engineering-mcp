#!/usr/bin/env node
// Persistent-session qualification driver for Inspector's production client.
//
// Inspector's published CLI is intentionally one request per process, while a
// ProjectRef and its Tasks authority are connection-local.  The Python gate
// therefore makes an attempt-local copy of the installed 2.5.0 CLI bundle and
// adds exports for the otherwise-private production InspectorClient and Node
// transport factory.  This file imports that copy and drives the unchanged
// implementation through one session.

import process from "node:process";
import { pathToFileURL } from "node:url";

const [bridgePath, serverArgvJson] = process.argv.slice(2);
if (!bridgePath || !serverArgvJson) {
  throw new Error("usage: m3-inspector-session.mjs BRIDGE SERVER_ARGV_JSON");
}
const serverArgv = JSON.parse(serverArgvJson);
if (!Array.isArray(serverArgv) || serverArgv.length === 0) {
  throw new Error("server argv must be a non-empty array");
}

const { InspectorClient, createTransportNode } = await import(
  pathToFileURL(bridgePath).href
);

const TASKS = "io.modelcontextprotocol/tasks";
const EXPECTED_TOOLS = [
  "rust.project.open",
  "rust.project.inspect",
  "rust.toolchain.inspect",
  "rust.check",
  "rust.fmt.check",
  "rust.clippy",
  "rust.test",
  "rust.test.nextest",
  "rust.dependencies.audit",
  "rust.diagnostics.explain",
  "rust.quality.gate",
  "rust.catalog.status",
  "rust.crate.search",
  "rust.crate.inspect",
  "rust.manifest.patch",
  "rust.fmt.apply",
  "rust.fix.apply",
  "rust.dependency.add",
  "rust.dependency.remove",
  "rust.coverage",
  "rust.semver.check",
  "rust.mutation.test",
];

function valuesFor(value, key, found = []) {
  if (Array.isArray(value)) {
    for (const child of value) valuesFor(child, key, found);
  } else if (value !== null && typeof value === "object") {
    for (const [name, child] of Object.entries(value)) {
      if (name === key) found.push(child);
      valuesFor(child, key, found);
    }
  }
  return found;
}

function uniqueString(value, key) {
  const values = [...new Set(valuesFor(value, key).filter((item) =>
    typeof item === "string"
  ))];
  if (values.length !== 1) {
    throw new Error(`${key} was absent or ambiguous`);
  }
  return values[0];
}

function toolByName(tools, name) {
  const tool = tools.find((candidate) => candidate.name === name);
  if (!tool) throw new Error(`tool is absent: ${name}`);
  return tool;
}

const serverConfig = {
  type: "stdio",
  command: serverArgv[0],
  args: serverArgv.slice(1),
  env: Object.fromEntries(
    ["HOME", "PATH", "TMPDIR", "USER", "LOGNAME", "SHELL",
      "RUST_MCP_TEST_SOCKET", "RUST_MCP_TEST_TASKS_READY"]
      .filter((name) => process.env[name] !== undefined)
      .map((name) => [name, process.env[name]])
  ),
};
const client = new InspectorClient(serverConfig, {
  environment: { transport: createTransportNode },
  clientIdentity: { name: "mcp-inspector", version: "2.5.0" },
  sample: false,
  elicit: false,
  progress: false,
  roots: [],
  advertisedExtensions: { [TASKS]: true },
  versionNegotiation: { mode: { pin: "2026-07-28" } },
  serverSettings: {
    protocolEra: "modern",
    connectionTimeout: 15_000,
    requestTimeout: 120_000,
  },
});

let cancellationTaskId;
let cancelSent = false;
client.addEventListener("requestorTaskUpdated", (event) => {
  const task = event.detail?.task;
  if (
    cancellationTaskId === "pending" &&
    task?.status === "working" &&
    typeof task.taskId === "string"
  ) {
    cancellationTaskId = task.taskId;
    if (!cancelSent) {
      cancelSent = true;
      void client.cancelRequestorTask(task.taskId);
    }
  }
});

try {
  await client.connect();
  if (client.getProtocolEra() !== "modern") {
    throw new Error("Inspector did not negotiate the modern protocol era");
  }
  if (client.getCapabilities()?.extensions?.[TASKS] === undefined) {
    throw new Error("server did not advertise the Tasks extension");
  }
  const { tools } = await client.listAllTools({ cacheMode: "refresh" });
  if (JSON.stringify(tools.map((tool) => tool.name)) !== JSON.stringify(EXPECTED_TOOLS)) {
    throw new Error("Inspector ordered inventory mismatch");
  }
  const openTool = toolByName(tools, "rust.project.open");
  const nextestTool = toolByName(tools, "rust.test.nextest");

  const passingOpen = await client.callTool(openTool, {
    path: process.env.RUST_MCP_M3_PASSING,
  });
  const passingRef = uniqueString(passingOpen.result, "project_ref");
  const passing = await client.callTool(nextestTool, {
    project_ref: passingRef,
    execution_mode: "task",
    timeout_seconds: 60,
  });
  if (passing.result?.structuredContent?.status !== "passed") {
    throw new Error("Inspector Tasks positive result did not pass");
  }
  const qualityUri = valuesFor(passing.result, "uri").find((value) =>
    typeof value === "string" && value.startsWith("rust-quality-artifact://")
  );
  if (!qualityUri) throw new Error("Inspector task result omitted its quality Resource");
  const resource = await client.readResource(qualityUri);
  if (!Array.isArray(resource.result?.contents) || resource.result.contents.length !== 1) {
    throw new Error("Inspector quality Resource read failed");
  }

  const failingOpen = await client.callTool(openTool, {
    path: process.env.RUST_MCP_M3_FAILING,
  });
  const failingRef = uniqueString(failingOpen.result, "project_ref");
  const failing = await client.callTool(nextestTool, {
    project_ref: failingRef,
    execution_mode: "task",
    timeout_seconds: 60,
  });
  if (failing.result?.structuredContent?.status !== "failed") {
    throw new Error("Inspector Tasks tool-failure result was not preserved");
  }

  const slowOpen = await client.callTool(openTool, {
    path: process.env.RUST_MCP_M3_SLOW,
  });
  const slowRef = uniqueString(slowOpen.result, "project_ref");
  cancellationTaskId = "pending";
  try {
    await client.callTool(nextestTool, {
      project_ref: slowRef,
      execution_mode: "task",
      timeout_seconds: 60,
    });
  } catch {
    // Inspector surfaces a cancelled terminal task as an exception today;
    // the authoritative oracle below is the subsequent server poll.
  }
  if (!cancelSent || cancellationTaskId === "pending") {
    throw new Error("Inspector did not cancel its active Tasks operation");
  }
  const cancelled = await client.getRequestorTask(cancellationTaskId);
  if (cancelled.status !== "cancelled") {
    throw new Error(`Inspector cancellation ended as ${cancelled.status}`);
  }

  process.stdout.write(`${JSON.stringify({
    version: "2.5.0",
    protocol_era: client.getProtocolEra(),
    tasks_declared: true,
    tasks_advertised: true,
    tool_count: tools.length,
    discovery: true,
    positive: true,
    failure: true,
    cancel: true,
    resource: true,
    task_flow: true,
  })}\n`);
} finally {
  await client.disconnect(5_000);
}
