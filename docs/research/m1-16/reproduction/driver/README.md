# Trusted M1-16 driver — review candidate, not measured evidence

Standalone Rust program, no workspace/source changes. It is an rmcp3.2.0 SDK client
and typed adapter for RustGateway, not an implementation of JSON-RPC/MCP framing.
Dependencies are pinned/path-based in Cargo.toml with separate offline Cargo.lock.
Binary after build: ../../target/debug/m1-16-trusted-driver from this directory.

## Trust boundary and IPC

The trusted controller owns stdin/stdout. This endpoint is NOT directly exposed to
an agent. First line is initialization, exactly once; host paths are absolute UTF8
with control characters rejected. Optional groups retain product constraints.

```json
{"mode":"raw","server_binary":"/absolute/rust-engineering-mcp","root":"/private/experiment/root","state_root":"/private/experiment/state","docker_socket":"/absolute/docker.sock"}
```

Use mode="mcp" for the actual product process. Optional keys: catalog_store,
catalog_trust, model_dir, index_store, rustsec_path, rustsec_sha256, stderr_path (an optional trusted log file, created
exclusively with mode0600 before launch; it must not already exist). Docker executable
and image are product constants; caller cannot set command/argv/env. Only the
configured root is supplied to the product. Product launch uses env_clear and no
shell. No assets acquired, no host Cargo execution on source files.

Wait for {"ready":true,"ipc_version":1,"server_pid":null,"negotiated_protocol":null}
before sending operations. MCP mode reports the owned product PID and negotiated
protocol instead of null, for trusted measurement/RSS attribution. Every line,
including newline, is bounded1MiB. Send one request, wait for its response; no
pipelining. Keep stdin open until the close acknowledgement. EOF is cancellation,
not successful completion, including EOF immediately after queued requests.

MCP mode operations:

```json
{"op":"tools"}
{"op":"call","name":"rust.project.open","arguments":{"path":"/private/experiment/root"}}
{"op":"call","name":"rust.check","arguments":{"project_ref":"prj_RETURNED_ID"}}
{"op":"resource","uri":"rust-artifact://prj_RETURNED_ID/art_RETURNED_ID"}
{"op":"close"}
```

Call tools first to complete product discovery. Only the13 M1 tool names are
accepted. Resource URI must contain valid product ProjectRef/ArtifactId. SDK result
payloads are serialized unchanged; SDK non-internal MCP errors return {"mcp_error":ErrorData}
with original code/message/data and permit subsequent requests. Internal errors
fail conservatively because they can represent cleanup/worker failures. Driver transport/validation failures use {"driver_error":"code"}.
There is no wrapper around successful SDK results and no handwritten MCP messages.
The session stays alive across operations, preserving references/resources.

Raw mode accepts only execute/close. Example schematic (controller supplies complete
immutable manifests/lock; abbreviated text below is not a runnable fixture):

```json
{"op":"execute","files":[{"path":"Cargo.toml","text":"..."},{"path":"Cargo.lock","text":"..."},{"path":"src/lib.rs","text":"..."}],"command":"check"}
```

Commands: check, fmt, clippy, test, metadata. Clippy selects Strict, test timeout30s;
ExecutionLimits30s and256KiB retained per stream. The existing domain caps wall at
60s; the experiment selects30s to match product validation defaults. Raw response is ExecutionResult,
including streams, truncation, termination, exit code and runtime fingerprints.
Files must include Cargo.toml, Cargo.lock, src/lib.rs, and may include ONLY
additional tests/behavior.rs. Duplicate/absolute/traversal/config/build.rs paths
are rejected before gateway initialization. SourceBundle enforces further bounds.
The controller separately enforces that manifests/locks and hidden tests are
immutable, that the hidden oracle is not shown to agents, and that model access
cannot reach this initialization/source submission channel directly.

Raw calibration is lazy, happens once per successful process generation and uses
only product trusted fixtures; all gateway work runs on a joined blocking worker.
No source file is executed by the driver host process. Source filesystem editing
for the MCP arm belongs to the separately reviewed controller broker, not this IPC.

## Cancellation, shutdown and limits

SIGINT/SIGTERM or IPC EOF permanently cancel the driver. Raw work receives the
shared atomic cancellation flag and is awaited until gateway cleanup completes.
MCP sends SDK cancellation for an in-flight request and closes the SDK transport,
then waits for its owned server child, even when service shutdown reports an error.
SDK request timeout900s also cancels via rmcp; handshake has30s timeout. No child
kill is used as successful cleanup. Close acknowledgement means execution owner
joined, not that a failed tool became successful or cleanup uncertainty was healed.

Output lines cap1MiB including newline. Oversized data fails; it is not silently
truncated into an apparently valid response. Each IPC write awaits up to5s. Native
or gateway cleanup may extend wall time: these are cooperative limits, not a hard
preemption claim. The runtime waits for execution owners before bounded shutdown;
the stdin/output blocking I/O tasks own no execution resources. Signal handling
currently targets Unix, matching the approved macOS host; no cross-platform claim.
Product stderr is drained concurrently: first256KiB retained, total count tracked,
and overflow explicitly reported. If stderr_path was configured, the retained
bytes are written to that trusted file; no raw stderr is sent to the agent channel.
Close includes stderr counts/truncation (null for raw). MCP log Resources remain
available through the SDK. Driver errors do not echo raw host paths/process errors.

## Initial implementation validation (historical)

- cargo generate-lockfile --offline:152 packages, locked independently.
- cargo check/build/test/clippy --all-targets --locked --offline, Cargo1.98.1,
  CARGO_INCREMENTAL=0, CARGO_TARGET_DIR=<LOCAL_HOME>/Projects/rust-mcp/target.
- Seven pure unit tests: closed IPC/config, source allowlist/duplicates, resource
  identity, fixed options/limits, bounded lines and permanent cancellation.
- smoke_no_execution.py: actual IPC rejects source traversal and unknown close
  fields, clean explicit close and SIGINT exit1. No valid execute request, MCP
  process, Docker command or supplied Rust execution is involved in this smoke.

Initial unit tests found serde unit variants accepted extra keys; empty-struct
variants now reject them. That failure is fixed, not hidden. Compilation found
non-exhaustive pinned SDK constructor requirements; public constructors are used.

## Initial implementation requirements (historical)

Independent security review, exact model/controller isolation, real gateway/MCP
same-session smoke, active cancellation/EOF/cleanup evidence, parity of command
budgets and source bytes, hidden oracle enforcement and protocolv2 freeze. No Docker
or live MCP/gateway run was performed during this implementation. This driver does
not establish experimental efficacy, M1 closure or production distribution readiness.

## Research bundle authoring

`research-bundle ABS_RECORDS_JSON ABS_PROVENANCE_JSON ABS_NEW_OUTPUT_DIR` is a
trusted host authoring binary, never an agent operation. It rehashes all retained
source evidence and annotation sources, verifies facts/labels/records/provenance
input digests, and reconstructs every projected field from the retained facts.
It preserves original unverified provenance and derives verified provenance only
for locally checked hashes, SQLite bytes and the public seed42 fixture signature.
This does not authenticate a real publisher or establish licensing approval.
`research-output/receipt.json` records scope, hashes and 69 checked source rows.
`baseline-projection.json` supplies the same derived provenance and actual SQLite
fingerprint to the raw arm. The fixture has 15 crates and 16 recorded versions.
Actual admin CLI import passed (`import-check-receipt.json`), without Docker/model.
One emitter test verifies its real signature and rejects changed container bytes.

Raw explain uses `{"op":"execute","command":"explain","code":"E0502","files":[]}`
with the same required source files as other commands (empty files here illustrate
only the envelope and are rejected). Code is required only for explain and is a
validated diagnostic code; no arbitrary rustc flags are accepted.
Final cancellation errors include `execution_joined` and `server_joined`. The
former is true only after owned execution workers finish; the latter requires an
actual successful child wait and is false for raw mode. These receipt fields do
not independently assert Docker cleanup; the controller must inspect that evidence.

## Resumed cleanup qualification

Current terminal acknowledgements include execution_joined, server_joined,
cleanup_uncertain and server_exit {code,signal,success} (null in raw mode).
Server join records a reaped process, separately from successful exit/cleanup.
The broker requires explicit cleanup_uncertain=false and all applicable joins;
only exact expected cancelled with exit1 is accepted. Any other error, missing
field, unsuccessful server exit or forced stop fails closed. Typed gateway cleanup
errors survive cancellation races. Production gateway cleanup is synchronous,
before execute returns, not a Drop action.

Pinned rmcp3.2.0 locally completes a cancelled response before server shutdown
may drain late replies. Keep a duplicated stdout endpoint during SDK close, then
drain it concurrently with child.wait/stderr so the product does not encounter a
broken pipe. SDK alone reads protocol during active work. No unsafe or new dependency;
constant8KiB drain buffer,1MiB shutdown total checked, nonzero exits still rejected.

13 driver tests and1 bundle test pass, plus fmt/Clippy/build. Actual final raw/MCP
cancellation on observed Cargo tests passed with joined cleanup and Docker object
absence; MCP server exit0. Initial failed qualification receipts are retained.
Exact source/binary identities and SDK source evidence are in
qualification/driver-cleanup-fix.md in the preserved research tree.
These are infrastructure qualifications, not utility measurements.
