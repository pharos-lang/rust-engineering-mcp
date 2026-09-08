#!/usr/bin/env node
// Persistent stock Inspector 2.5.0 qualification for the five M4 tools.
// Python creates the private fixtures and an attempt-local export-only bridge.
import process from "node:process";
import { pathToFileURL } from "node:url";

const [bridgePath, serverArgvJson] = process.argv.slice(2);
if (!bridgePath || !serverArgvJson) throw new Error("usage: m4-inspector-session.mjs BRIDGE SERVER_ARGV_JSON");
const serverArgv = JSON.parse(serverArgvJson);
if (!Array.isArray(serverArgv) || serverArgv.length === 0) throw new Error("server argv must be non-empty");
const targetIndex = serverArgv.indexOf("--server-argv-json");
if (targetIndex < 0) throw new Error("qualification proxy target missing");
const targetArgv = JSON.parse(serverArgv[targetIndex + 1]);
if (!Array.isArray(targetArgv)) throw new Error("qualification target argv invalid");
const { InspectorClient, createTransportNode } = await import(pathToFileURL(bridgePath).href);

const TASKS = "io.modelcontextprotocol/tasks";
const EXPECTED_TOOLS = [
  "rust.project.open", "rust.project.inspect", "rust.toolchain.inspect", "rust.check",
  "rust.fmt.check", "rust.clippy", "rust.test", "rust.test.nextest",
  "rust.dependencies.audit", "rust.diagnostics.explain", "rust.quality.gate",
  "rust.catalog.status", "rust.crate.search", "rust.crate.inspect", "rust.manifest.patch",
  "rust.fmt.apply", "rust.fix.apply", "rust.dependency.add", "rust.dependency.remove",
  "rust.coverage", "rust.semver.check", "rust.mutation.test", "rust.deny",
  "rust.unsafe.scan", "rust.supply_chain.inspect", "rust.quality.gate.v2", "rust.miri",
];
const M4 = ["rust.deny", "rust.unsafe.scan", "rust.supply_chain.inspect", "rust.quality.gate.v2", "rust.miri"];

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

const serverConfig = {
  type: "stdio", command: serverArgv[0], args: serverArgv.slice(1),
  env: Object.fromEntries(["HOME", "PATH", "TMPDIR", "USER", "LOGNAME", "SHELL", "RUST_MCP_TEST_SOCKET"]
    .filter((name) => process.env[name] !== undefined).map((name) => [name, process.env[name]])),
};
const client = new InspectorClient(serverConfig, {
  environment: { transport: createTransportNode },
  clientIdentity: { name: "mcp-inspector", version: "2.5.0" },
  sample: false, elicit: false, progress: false, roots: [],
  advertisedExtensions: { [TASKS]: true },
  versionNegotiation: { mode: { pin: "2026-07-28" } },
  serverSettings: { protocolEra: "modern", connectionTimeout: 15_000, requestTimeout: 600_000 },
});

let cancellationTaskId;
let cancelSent = false;
client.addEventListener("requestorTaskUpdated", (event) => {
  const task = event.detail?.task;
  if (cancellationTaskId === "pending" && task?.status === "working" && typeof task.taskId === "string") {
    cancellationTaskId = task.taskId;
    if (!cancelSent) { cancelSent = true; void client.cancelRequestorTask(task.taskId); }
  }
});

try {
  await client.connect();
  if (client.getProtocolEra() !== "modern") throw new Error("Inspector did not negotiate modern MCP");
  if (client.getCapabilities()?.extensions?.[TASKS] === undefined) throw new Error("server omitted Tasks");
  const { tools } = await client.listAllTools({ cacheMode: "refresh" });
  if (JSON.stringify(tools.map((tool) => tool.name)) !== JSON.stringify(EXPECTED_TOOLS)) {
    throw new Error("Inspector ordered inventory mismatch");
  }
  const rootIndex = targetArgv.indexOf("--root");
  if (rootIndex < 0 || typeof targetArgv[rootIndex + 1] !== "string") throw new Error("private fixture root missing");
  const opened = await client.callTool(toolByName(tools, "rust.project.open"), { path: targetArgv[rootIndex + 1] });
  const projectRef = uniqueString(opened.result, "project_ref");
  let resourceCount = 0;
  for (const name of M4) {
    const args = { project_ref: projectRef, execution_mode: "task", timeout_seconds: name === "rust.quality.gate.v2" ? 600 : 120 };
    if (name === "rust.quality.gate.v2") args.profile = "strict";
    const result = await client.callTool(toolByName(tools, name), args);
    if (result.result?.structuredContent?.status !== "passed") throw new Error(`Inspector positive ${name} did not pass`);
    const uris = valuesFor(result.result, "uri").filter((value) => typeof value === "string" && value.startsWith("rust-quality-artifact://"));
    if (uris.length === 0) throw new Error(`Inspector ${name} omitted Resource`);
    for (const uri of uris) {
      const resource = await client.readResource(uri);
      if (!Array.isArray(resource.result?.contents) || resource.result.contents.length !== 1) throw new Error(`Resource failed for ${name}`);
      resourceCount += 1;
    }
  }
  for (const name of M4) {
    const args = { project_ref: `prj_${"0".repeat(32)}`, execution_mode: "task", timeout_seconds: 60 };
    if (name === "rust.quality.gate.v2") args.profile = "strict";
    let rejected = false;
    try {
      const result = await client.callTool(toolByName(tools, name), args);
      rejected = result.result?.isError === true && ["blocked", "unavailable"].includes(result.result?.structuredContent?.status);
    } catch (error) {
      // Tasks cannot bind an unknown ProjectRef; admission is a masked RPC error.
      rejected = error?.code === -32602 && error.message.includes("task unavailable");
    }
    if (!rejected) {
      throw new Error(`Inspector negative ${name} was not preserved`);
    }
  }
  const cancelRootIndex = targetArgv.indexOf("--root", rootIndex + 1);
  if (cancelRootIndex < 0 || typeof targetArgv[cancelRootIndex + 1] !== "string") throw new Error("cancel fixture root missing");
  const cancelOpen = await client.callTool(toolByName(tools, "rust.project.open"), { path: targetArgv[cancelRootIndex + 1] });
  const cancelRef = uniqueString(cancelOpen.result, "project_ref");
  cancellationTaskId = "pending";
  try {
    await client.callTool(toolByName(tools, "rust.miri"), { project_ref: cancelRef, execution_mode: "task", timeout_seconds: 120 });
  } catch { /* the terminal cancelled state is checked below */ }
  if (!cancelSent || cancellationTaskId === "pending") throw new Error("Inspector did not issue Tasks cancellation");
  const cancelled = await client.getRequestorTask(cancellationTaskId);
  if (cancelled.status !== "cancelled") throw new Error(`Inspector cancellation ended as ${cancelled.status}`);
  process.stdout.write(`${JSON.stringify({version:"2.5.0",protocol_era:"modern",tool_count:tools.length,
    m4_tool_count:M4.length,discovery:true,positive:true,negative:true,cancel:true,resource:true,
    resources_read:resourceCount,task_flow:true,tasks_declared:true,tasks_advertised:true})}\n`);
} finally {
  await client.disconnect(5_000);
}
