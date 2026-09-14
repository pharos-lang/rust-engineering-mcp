#!/usr/bin/env node
// Stock Inspector 2.5.0 qualification for the five M6 analyzer tools.
//
// Python owns every decision: it builds the attempt-local export-only bridge,
// the closed server argv and the whole call plan (arguments, the exact declared
// status/error_code each call must produce). This file only drives the stock
// client and reports the oracles it observed, so no expectation can drift
// between the harness and the session.
//
// The same driver serves both modes. In the Docker-free mode every planned
// call is SANDBOX_DENIED, produced before any container exists. In the
// runtime mode it performs real analyzer sessions in the qualified M6 image
// and drives the full `rust.analyzer.action.apply` preview -> commit ->
// reopen -> receipt -> stale-preview lifecycle against a temporary copy of a
// fixture. Unlike M3-M5, none of these five tools publish an MCP Resource, so
// no Resource oracle runs here.
import process from "node:process";
import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const [bridgePath, serverArgvJson, planJson] = process.argv.slice(2);
if (!bridgePath || !serverArgvJson || !planJson) {
  throw new Error("usage: m6-inspector-session.mjs BRIDGE SERVER_ARGV_JSON PLAN_JSON");
}
const serverArgv = JSON.parse(serverArgvJson);
if (!Array.isArray(serverArgv) || serverArgv.length === 0) throw new Error("server argv must be non-empty");
const plan = JSON.parse(planJson);
if (!Array.isArray(plan.expected_tools) || plan.expected_tools.length === 0) throw new Error("expected inventory missing");
if (!Array.isArray(plan.calls) || plan.calls.length === 0) throw new Error("call plan missing");
if (plan.projects === null || typeof plan.projects !== "object") throw new Error("project plan missing");
const { InspectorClient, createTransportNode } = await import(pathToFileURL(bridgePath).href);

const RUNTIME = "runtime";

function stable(value) {
  if (Array.isArray(value)) return value.map(stable);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort((left, right) => (left < right ? -1 : left > right ? 1 : 0)).map((name) => [name, stable(value[name])]));
  }
  return value;
}
function measure(value) {
  const sorted = stable(value);
  const bytes = Buffer.from(JSON.stringify(sorted === undefined ? null : sorted), "utf8");
  return { bytes: bytes.length, sha256: createHash("sha256").update(bytes).digest("hex") };
}
function toolByName(tools, name) {
  const tool = tools.find((candidate) => candidate.name === name);
  if (!tool) throw new Error(`tool absent: ${name}`);
  return tool;
}
function fileSha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

// Assemble one call's arguments from the plan row and the session's own
// captured state; the digest, plan identity and operation id are never
// hard-coded, only ever carried forward from an earlier response.
function assembleArguments(row, refs, state) {
  const reference = row.project === "write" ? refs.write : refs[row.project];
  if (typeof reference !== "string") throw new Error(`project authority missing for ${row.tool}`);
  const kind = row.kind ?? "static";
  if (kind === "static") return { project_ref: reference, ...row.arguments };
  if (kind === "docker_free_fingerprint") {
    const fingerprint = state.fingerprints[row.project];
    if (typeof fingerprint !== "string") throw new Error(`no captured fingerprint for ${row.project}`);
    if (row.tool === "rust.analyzer.actions") {
      return { project_ref: reference, expected_project_fingerprint: fingerprint, ...row.arguments };
    }
    return {
      project_ref: reference,
      action: { expected_project_fingerprint: fingerprint, ...row.arguments.action },
    };
  }
  if (kind === "actions_capture") {
    return {
      project_ref: reference, expected_project_fingerprint: state.preFingerprint,
      ...row.arguments,
    };
  }
  if (kind === "apply_preview") {
    return {
      project_ref: reference,
      action: {
        mode: "preview", expected_project_fingerprint: state.preFingerprint,
        action_digest: state.actionDigest, file: plan.analyzer_file, range: plan.actions_range,
      },
    };
  }
  if (kind === "apply_commit") {
    return {
      project_ref: reference,
      action: { mode: "commit", plan_id: state.planId, plan_digest: state.planDigest, ...row.arguments },
    };
  }
  if (kind === "apply_receipt") {
    return {
      project_ref: reference,
      action: { mode: "receipt", operation_id: state.planId, ...row.arguments },
    };
  }
  if (kind === "apply_preview_stale") {
    return {
      project_ref: reference,
      action: {
        mode: "preview", expected_project_fingerprint: state.postFingerprint,
        action_digest: state.actionDigest, file: plan.analyzer_file, range: plan.actions_range,
      },
    };
  }
  throw new Error(`unknown row kind: ${kind}`);
}

function captureActionsFacts(structured, state) {
  const actions = structured?.data?.actions;
  if (!Array.isArray(actions) || actions.length === 0) throw new Error("no actions listed");
  const first = actions[0];
  if (first?.applicability !== "applicable" || !/^sha256:[0-9a-f]{64}$/.test(first.action_digest ?? "")) {
    throw new Error("the first listed action is not applicable with a real digest");
  }
  state.actionDigest = first.action_digest;
}
function capturePreviewFacts(structured, state) {
  const data = structured?.data;
  if (data?.kind !== "preview" || !Array.isArray(data.files) || data.files.length === 0) {
    throw new Error("apply preview published no reviewable diff");
  }
  if (!data.files.some((change) => change.before_sha256 !== change.after_sha256)) {
    throw new Error("apply preview's diff changes nothing");
  }
  if (!/^mut_[0-9a-f]{32}$/.test(data.plan_id ?? "") || !/^sha256:[0-9a-f]{64}$/.test(data.plan_digest ?? "")) {
    throw new Error("apply preview published no valid plan identity");
  }
  state.planId = data.plan_id;
  state.planDigest = data.plan_digest;
}
function checkReceiptFacts(structured, state) {
  const data = structured?.data;
  if (data?.kind !== "receipt" || data.state !== "committed" || data.operation_id !== state.planId) {
    throw new Error("apply receipt did not report a committed operation");
  }
}

// rust-analyzer's code actions are not deterministically available from a
// transient per-query instance (ADR-084): the `serverStatus` quiescent
// readiness this gateway waits on does not guarantee assists are indexed
// yet. This one row is read-only and idempotent, so a bounded retry of the
// identical call is safe and makes the Inspector's write-lifecycle
// demonstration reliable against that race; see W09d.
const ACTIONS_CAPTURE_RETRIES = 3;
const ACTIONS_CAPTURE_RETRY_DELAY_MS = 1500;

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function actionsCaptureEmpty(structured) {
  const data = structured?.data;
  return Array.isArray(data?.actions) && data.actions.length === 0
    && data?.completeness?.state === "complete";
}

async function performCall(client, tools, row, args) {
  const request = measure(args);
  let result;
  try {
    result = (await client.callTool(toolByName(tools, row.tool), args)).result;
  } catch (error) {
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
  return { request, result, structured };
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

  // One authority per fixture/project root, resolved once and reused (the
  // write project is reopened, in place, after its commit row). Every open's
  // fingerprint is captured too: the Docker-free `actions`/`action.apply`
  // rows need theirs, exactly as the write lifecycle needs the write
  // project's.
  const refs = {};
  const fingerprints = {};
  let preFingerprint = null;
  for (const [name, path] of Object.entries(plan.projects)) {
    if (typeof path !== "string" || path.length === 0) throw new Error(`fixture root missing: ${name}`);
    const opened = await client.callTool(toolByName(tools, "rust.project.open"), { path });
    if (opened.result?.isError === true) throw new Error(`Inspector could not open ${name}`);
    const data = opened.result?.structuredContent?.data;
    if (typeof data?.project_ref !== "string") throw new Error(`Inspector open for ${name} returned no project_ref`);
    refs[name] = data.project_ref;
    if (typeof data.fingerprint !== "string") throw new Error(`Inspector open for ${name} returned no fingerprint`);
    fingerprints[name] = data.fingerprint;
    if (name === "write") preFingerprint = data.fingerprint;
  }
  const state = { preFingerprint, fingerprints };

  const rows = [];
  let writeVerifiedOnDisk = null;
  let preWriteBytesSha256 = null;
  if (typeof plan.projects.write === "string") {
    preWriteBytesSha256 = fileSha256(`${plan.projects.write}/src/lib.rs`);
  }
  for (const row of plan.calls) {
    const args = assembleArguments(row, refs, state);
    const kind = row.kind ?? "static";
    let request, result, structured;
    if (kind === "actions_capture") {
      for (let attempt = 1; ; attempt += 1) {
        ({ request, result, structured } = await performCall(client, tools, row, args));
        if (!actionsCaptureEmpty(structured)) break;
        if (attempt >= ACTIONS_CAPTURE_RETRIES) {
          throw new Error(
            `analyzer offered no assist after ${ACTIONS_CAPTURE_RETRIES} retries — `
            + "known assist-readiness race, W09d"
          );
        }
        await sleep(ACTIONS_CAPTURE_RETRY_DELAY_MS);
      }
    } else {
      ({ request, result, structured } = await performCall(client, tools, row, args));
    }
    if (kind === "actions_capture") captureActionsFacts(structured, state);
    else if (kind === "apply_preview") capturePreviewFacts(structured, state);
    else if (kind === "apply_receipt") checkReceiptFacts(structured, state);
    if (row.reopen_after === true) {
      const reopened = await client.callTool(toolByName(tools, "rust.project.open"), { path: plan.projects.write });
      const data = reopened.result?.structuredContent?.data;
      if (typeof data?.project_ref !== "string" || data.project_ref === refs.write
          || typeof data?.fingerprint !== "string") {
        throw new Error("Inspector did not obtain a fresh write reference after commit");
      }
      refs.write = data.project_ref;
      state.postFingerprint = data.fingerprint;
      if (preWriteBytesSha256 !== null) {
        const afterCommitSha256 = fileSha256(`${plan.projects.write}/src/lib.rs`);
        writeVerifiedOnDisk = afterCommitSha256 !== preWriteBytesSha256;
      }
    }

    const response = measure(result);
    rows.push({
      client: "inspector", tool: row.tool, shape: row.shape, mode: row.mode,
      status: structured.status, error_code: structured.error_code ?? null,
      is_error: result?.isError === true,
      request_bytes: request.bytes, request_sha256: request.sha256,
      response_bytes: response.bytes, response_sha256: response.sha256,
    });
  }

  // G4: one analyzer call, cancelled mid-flight over the real runtime, is
  // reported through the SDK's own non-Tasks cancellation as a rejected
  // pending call; a fresh, uncancelled retry of the identical call then
  // succeeding is the evidence that the server's cleanup joined cleanly and
  // left the session healthy.
  let cancelOk = null;
  if (plan.mode === RUNTIME) {
    const diagnosticsRow = plan.calls.find((row) => row.fact_key === "diagnostics");
    if (diagnosticsRow === undefined) throw new Error("no diagnostics row to exercise cancellation on");
    const args = assembleArguments(diagnosticsRow, refs, state);
    const pending = client.callTool(toolByName(tools, diagnosticsRow.tool), args);
    const cancelled = client.cancelToolCall();
    if (!cancelled) throw new Error("Inspector had no in-flight call to cancel");
    let rejected = false;
    try {
      await pending;
    } catch {
      rejected = true;
    }
    if (!rejected) throw new Error("Inspector's cancelled call resolved instead of rejecting");
    const retried = await client.callTool(toolByName(tools, diagnosticsRow.tool), args);
    cancelOk = retried.result?.isError !== true
      && retried.result?.structuredContent?.status === "passed";
    if (!cancelOk) throw new Error("Inspector's post-cancel retry did not observe joined cleanup");
  }

  process.stdout.write(`${JSON.stringify({
    version: "2.5.0", protocol_era: "modern", mode: plan.mode, tool_count: names.length,
    m6_tool_count: plan.m6_tools.length, discovery: true,
    positive_calls: rows.filter((row) => row.shape === "positive").length,
    negative_calls: rows.filter((row) => row.shape === "negative").length,
    write_verified_on_disk: plan.mode === RUNTIME ? writeVerifiedOnDisk === true : null,
    cancel_ok: plan.mode === RUNTIME ? cancelOk === true : null,
    calls: rows,
  })}\n`);
} finally {
  await client.disconnect(5_000);
}
