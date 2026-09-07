# ADR-060 — Bounded job execution and negotiated MCP Tasks

## Status

Accepted 2026-09-06 by the M3 orchestrator after independent reviews V06/V17/V18
(ADR-063: owner provisioning authorization 2026-09-05). Implemented and qualified
for M3-02; see `docs/validation/M3-02.md`.

## Context

M3 introduces quality operations whose useful execution time can exceed a normal
`tools/call` exchange. The operation still has to cross the same closed Execution
Gateway as M1/M2, treat all project code as hostile, preserve the one-active-worker
admission rule, and terminate and join the process tree before capacity is reused.
An asynchronous protocol shape must not weaken those properties or make an opaque
identifier into authority.

The Tasks extension in pinned `rmcp 3.2.0` is usable but is not a complete job
runtime. With the `server` feature, `ServerCapabilities::builder().enable_tasks()`
emits `capabilities.extensions["io.modelcontextprotocol/tasks"]`. The SDK requires
the peer to declare the same extension for `tasks/get`, `tasks/update`, and
`tasks/cancel`; otherwise it returns `-32021`. A handler may return
`CallToolResponse::Task(CreateTaskResult)`, and the client polls `tasks/get` until a
terminal payload. There is no `tasks/result` or `tasks/list` method.

The SDK does not gate the extension on protocol version. The wire spike associated
with this ADR observes it on all five protocol versions supported by the product,
including a `2024-11-05` peer which declares the extension. It also observes that
`ttlMs: None` serializes as `null` and that `tasks/cancel` acknowledges cooperative
cancellation while the task may remain `working`. In this SDK version,
`ProtocolVersion::LATEST` for legacy initialization is `2025-11-25`; the
`2026-07-28` lifecycle uses `server/discover` and per-request metadata.

`rmcp::TaskManager` is intentionally generic: it is in-memory, generates UUIDs,
spawns Tokio tasks, performs opportunistic TTL sweeps, permits unlimited lifetime,
echoes an unknown task identifier in its error, and aborts futures at expiry or
shutdown. It neither owns the Execution Gateway's process-tree join proof nor binds
a task to a host-authorized owner and live `ProjectRef`. Its cancellation context is
also independent of the originating request's `CancellationToken`. Those facts make
it useful spike evidence, not the product lifecycle authority.

ADR-030 additionally retains one of the 16 request leases when rmcp suppresses a
cancelled response. rmcp drops that response before the transport sink, while the
existing `AdmittedTransport::send` boundary can observe successful send completion.
A task can therefore be committed while its `CreateTaskResult` is never handed to
the sink. D06 must use that delivery evidence to bound the orphan race without
releasing either request or worker capacity prematurely.

## Decision

### Neutral domain contract

Add Rust domain/application types before an M3 tool is exposed. None contains an
`rmcp`, JSON-RPC, Tokio, Cargo, SQLite, LanceDB, or transport type:

- `JobId` is an unguessable 128-bit random value encoded canonically as
  `job_` plus 32 lowercase hexadecimal digits. It has no embedded owner, path,
  project reference, timestamp, sequence, or policy fact.
- `JobKind` is a closed enum of qualified application operations. Arbitrary command
  names and arguments are not representable.
- `JobOwner` uses the same owner boundary as ADR-061: a domain-separated SHA-256
  binding of state-root device/inode, host uid, granted-root device/inode, and the
  host-granted project workspace-root string, paired with current `ProjectRef`
  liveness. The stdio session is process-constant and adds no discrimination.
  Client name, peer/session IDs, fingerprints, `ProjectRef` strings and locators are
  excluded from the binding and are never principals or grants. Authorization and
  policy generations remain separate facts revalidated on every operation.
- `JobState` is `Admitted`, `Running`, `Completed`, `Failed`, or `Cancelled`.
  Cancellation intent is separate control state, not a terminal state.
- `JobPhase` is the closed set `Admission`, `Capture`, `Prepare`, `Execute`,
  `Collect`, `Publish`, and `Cleanup`. Phase and state are orthogonal: a cancellation
  request normally leaves the wire status `working` while `Cleanup` is in progress.
- `JobBudget`, `JobDeadline`, `ResultRetention`, and quota types carry explicit units
  and checked values. Deadlines use a monotonic clock; timestamps exposed on the wire
  are observational UTC timestamps and never authorize work.
- `JobCompletion` contains a typed application result or a closed infrastructure
  failure. The MCP adapter alone maps it to `CallToolResult`/`ErrorData`. An ordinary
  complete tool outcome, including `isError: true`, maps to MCP `completed`; an
  infrastructure failure maps to `failed`.

M3 initially has no `InputRequired` domain state. Interactive input is neither an
authority channel nor required by the four planned M3 tools.

### Application executor and registry

Application owns a `JobExecutor` service with `submit`, `status`, `cancel`,
`update`, and `shutdown_and_join` operations. Its narrow ports are a transactional
owner-bound `JobRegistry`, a monotonic clock/ID source, and the already closed joined
execution boundary. The registry stores lifecycle metadata and bounded terminal
results; it does not execute work. The Execution Gateway remains the only route to
an external process.

Submission is fail-closed and ordered:

1. validate the typed tool input, budget, current host grant, `ProjectRef`, physical
   identity and policy;
2. select and validate the execution mode against negotiated capabilities and the
   existing `ready` bootstrap gate;
3. non-blockingly acquire the existing ADR-030 worker permit; there is no waiting
   queue, and busy rejects before any result/task reservation;
4. reserve the bounded task-record/result quota;
5. create the owner-bound record and start the joined operation as one application
   commit, rolling back the reservation and joining any started work on failure;
6. return the seed only after the record is observable.

The job permit **is the existing ADR-030 `Workers::run_joined` permit**, not a
second semaphore or worker pool. It is held through execution, process-tree cleanup,
result normalization, authority revalidation, and terminal publication. It is
released only after cleanup is positively observed. Cleanup uncertainty dominates a
useful tool result: the task stays `working` with phase `Cleanup`, the session is
quarantined and shut down, and no new job is admitted. If containment is later
proved, the task becomes `failed`; otherwise no false terminal state or capacity
release is published.

`status`, `cancel`, and `update` never acquire the job permit. They still use the
ordinary ADR-030 request admission lease, perform bounded registry work, and cannot
start execution. While an asynchronous job holds the worker permit, every other
`tools/call` and every M1 Resource read that uses `run_joined` returns the existing
busy/`SANDBOX_DENIED` response. Quality Resource reads are deliberately off that
permit: after the shared `ready` bootstrap gate they use only non-blocking live
authority/store acquisition, mapping contention to the same `Resource not found`
outcome as absence. Task artifact-liveness uses the same fail-fast rule and projects
contention as `Unavailable`. This 2026-09-06 V-SEC amendment preserves
`tasks/get`, `tasks/cancel`, watchdog and stdin/EOF availability while hostile code
runs. Task registry controls likewise remain exempt.

The fail-fast authority projection while the registry is contended depends on
ADR-030's single-permit invariant: the job holding that permit is the only possible
registry holder during execution, so a second revoking mutation cannot run
concurrently. Any future admission path that introduces another registry writer
must reopen this decision instead of treating contention as authorization.

Cancellation is idempotent: it persists intent, signals the joined operation and
returns an acknowledgement. `Cancelled` is published only after the gateway confirms
termination, pipe drain, cleanup, and join. A late successful operation after a
cancellation request remains `Completed`; a real failure remains `Failed` rather
than being masked.

An independent watchdog enforces non-delivery, execution, cleanup and retention
deadlines even when nobody polls. It can only signal the same cooperative control
tokens and await the real joined worker; it never aborts a Tokio task or claims to
preempt a blocking closure. Expiry is not opportunistic. Polling never extends a
deadline or TTL. A non-terminal expiry requests cancellation and joins cleanup;
only then is a bounded timeout failure published. Terminal records consume result
retention quota but not the job permit. Artifact persistence and TTL are separate:
a retained job result may hold authorized artifact references. Those references are
evidence, not a dangling-reference promise: an expired member resolves as
`Resource not found`, and `tasks/get` projects that member as `Unavailable` at read
time without changing the historical task outcome. Neither a record nor an artifact
makes execution durable.

### MCP adapter and authority

Implement `ServerHandler::get_task`, `cancel_task`, and `update_task` over the
application registry. Use rmcp for negotiation, typed messages, dispatch and wire
serialization, but do **not** wrap `rmcp::TaskManager` in product code.

Every task method first passes rmcp's negotiated capability check, then looks up the
opaque ID and revalidates the derived owner binding, live `ProjectRef`, physical
identity, current host grant and policy generation. The ID alone is never sufficient.
Unknown, malformed, expired, cross-ProjectRef, cross-grant, revoked-project and
policy-invalid IDs all return the same `-32602` response with the fixed message
`task unavailable`, no identifier echo and no distinguishable metadata. Cross-session
IDs are already unreachable because the registry is process-local; the discriminating
foreign cases are cross-ProjectRef and cross-grant within the process. If revocation
is discovered for a running job, cancellation and joined cleanup begin, while the
caller still receives the masked response. Publication revalidates the same facts and
discards inaccessible result content. There is no global enumeration.

Every operation retains the existing gateway invariants: arguments and environment
come only from closed typed allowlists, host environment is not inherited, network
deny requires actual sandbox enforcement, and server-owned filesystem access is
handle-relative and no-follow. A job or task mode never authorizes a shell, free
flags, host execution, downloads, or weaker containment.

`update_task` applies the same masked lookup and authority checks, then returns fixed
`-32602 task does not accept input` for an authorized M3 job because M3 never enters
`input_required`. It cannot grant authority or change a job's command, budget,
project, policy or owner. A future interactive job requires a separate ADR and
domain state before this behavior changes.

`tools/call` returns `CreateTaskResult` with `working`, a fixed non-null `ttlMs`, a
fixed suggested poll interval and a low-sensitivity status. `tasks/get` returns the
current flattened rmcp shape; the terminal application result is inline in
`result`. `tasks/cancel` only acknowledges recorded intent. There is no invented
`tasks/result`, `tasks/list`, custom JSON-RPC parser, or parallel protocol stack.
Although rmcp has a `notifications/tasks` model, its subscription router does not
route that method in 3.2.0; M3 uses polling only and makes no notification promise.

Status messages come from a closed table such as `admitted`, `capturing project`,
`preparing execution`, `executing`, `collecting evidence`, `publishing result`, and
`cleaning up`. They never contain paths, source, symbols, diagnostics, arguments,
environment values, project references, policies, client data, or raw tool output.

### Negotiation and synchronous fallback

Static advertisement on every supported version remains the chosen policy, but it
is not forced by rmcp. `ServerHandler::initialize` and `discover` are overridable and
can tailor their response using the peer request/context while `get_info()` retains
Tasks for SDK capability validation. The static policy is simpler and makes server
support discoverable even when a peer does not declare the extension:

| Negotiated version | Lifecycle | Server advertisement | Task use |
|---|---|---|---|
| `2024-11-05` | `initialize` | advertise | only if client declared the extension at initialize |
| `2025-03-26` | `initialize` | advertise | only if client declared the extension at initialize |
| `2025-06-18` | `initialize` | advertise | only if client declared the extension at initialize |
| `2025-11-25` | `initialize` | advertise | only if client declared the extension at initialize |
| `2026-07-28` | `server/discover`/inline metadata | advertise | only on calls whose per-request capabilities declare it |

This table records observed rmcp behavior, not a claim that every legacy client
understands the extension. Actual task materialization and all task methods are
gated by the client's declaration. The static advertisement flip is gated on
recorded G4 evidence: the repository protocol harness must pass the five versions ×
declared/not-declared matrix, and MCP Inspector 2.5.0 plus stock model-driven Codex
CLI 0.153.0 must pass their actual negotiated paths against the same candidate.
That evidence must include peers which do not declare Tasks and prove synchronous
fallback rather than inferring compatibility from ignored extension fields. The G4
gate passed on 2026-09-06: Inspector 2.5.0 declared Tasks and completed the task
lifecycle, while Codex app-server 0.153.0 did not declare it and completed its
synchronous/model-directed path. The production switch is therefore enabled.

If either stock client rejects `extensions`, the accepted remediation is to override
`initialize`/`discover` and omit Tasks from that response unless the peer declared
it, under a host-approved compatibility policy; then rerun the matrix. Client name
alone is not trusted as an authority or compatibility grant. If neither available
stock client declares Tasks, the repository protocol harness qualifies the Tasks
path, the limitation is documented explicitly, and Inspector/Codex qualify the
synchronous path. Before this complete evidence exists, the product does not
advertise Tasks.

Only new M3 tool schemas add `execution_mode` with closed values `auto`, `task`, and
`synchronous`, defaulting to `auto`. The existing 18 tools keep their schemas and
semantics byte for byte.

- `task` requires the peer Tasks capability. Its absence makes the typed input
  contradict the negotiated session and returns fixed `-32602` with no data, before
  worker admission, reservation or work.
- `synchronous` waits for the same neutral `JobExecutor` under the explicit
  synchronous work budget. It is rejected before any effect if the operation's
  qualified worst-case bound exceeds the requested budget.
- `auto` selects Tasks when mutually declared; otherwise it selects synchronous only
  for an operation qualified as short. `auto` intentionally never selects
  synchronous for a Tasks-capable peer. A long or unqualified operation returns a
  structured `isError` result with closed remediation code `TASKS_REQUIRED`, before
  worker admission, reservation or execution.

Every M3 tool reuses the existing `ready` bootstrap gate: an expensive M3 operation
cannot be the first request on the rmcp bootstrap path and receives the existing
structured denial. For a second `tools/call` while a job is active, the exact order
is input validation, mode selection, then busy rejection before reservation; busy
uses ADR-040's existing conservative `SANDBOX_DENIED` structured contract regardless
of task or synchronous mode. The existing 10-second bootstrap behavior, 1 MiB stdio
frame caps, 16 request slots and cancelled-response retention remain unchanged.
Sync fallback does not widen them.

### Admission leases and the publication race

Every `tools/call` and `tasks/*` request uses its existing ADR-030 request lease.
After an asynchronous submission is committed, the active job retains that same
ADR-030 worker permit; poll/cancel/update never acquire it. A normally delivered
`CreateTaskResult` lets the request admission lease complete while the worker permit
remains held until joined terminal publication.

Cancellation before the submission commit point rolls back or cancels and joins the
candidate job. At commit, the registry records the originating `RequestContext.id`
and owns a cancellation token seeded **only** from the session shutdown token.
`RequestContext::ct` is never passed to a job which outlives that request: rmcp
cancels it when the response is queued for the sink, so following the existing
short-call pattern would cancel every task-mode job immediately after creation.
After commit, only `tasks/cancel`, the registry watchdog, deadline/expiry,
revocation, or session shutdown controls the job.

Delivery state is keyed by that request ID. `AdmittedTransport` marks the job
`Delivered` only after successful response send completion. A response suppressed by
rmcp is never handed to `AdmittedTransport::send`; only that `NeverHandedToSend`
state receives the fixed 30-second non-delivery deadline. A send failure triggers
session cancellation and joined shutdown, not an inference of delivery. A delivered
but never-polled job runs under its normal work/cleanup deadlines and fixed TTL; poll
frequency is not an ownership or liveness signal.

On the non-delivery deadline, the watchdog cooperatively cancels and joins the orphan
without recycling ADR-030's conservatively retained request lease. After cleanup,
the legitimate owner who later polls the known ID sees terminal `cancelled`, not
masked `-32602`, until record expiry. After 16 suppressed request responses the
existing policy still closes the session.

On EOF, `JobExecutor::shutdown_and_join` is awaited inside the same current-thread
runtime `block_on` closure. It signals and joins all jobs before
`workers.shutdown(grace)` may return its verdict and before `shutdown_timeout` is
called. Neither EOF nor the watchdog aborts a detached Tokio future as a substitute
for the real blocking-worker/process-tree join.

### Measured budgets and quotas (M3-02)

These are the accepted M3-02 limits. A smaller existing tool, gateway, image,
cgroup, project lease or policy limit always wins; none silently expands.

| Unit | Phase/boundary | Default | Maximum | Exceed behavior |
|---|---|---:|---:|---|
| active job | admission | 1 per stdio session, no queue | 1 | reject busy before record creation or execution |
| seed commit | admission + start | 5 s | 5 s | roll back reservation; cancel/join if anything started |
| async job work | capture through publish, excluding cleanup | 300 s | 3,600 s | request cancellation; enter joined cleanup |
| capture + prepare | pre-execution sub-deadline | 60 s | 120 s | cancel before project execution where possible; join cleanup |
| execute | gateway sub-deadline | 180 s | 3,360 s | gateway terminates tree; join; publish timeout only after cleanup |
| collect + publish | post-execution sub-deadline | 30 s | 120 s | fail closed; discard unpublishable result; cleanup still wins |
| cleanup | separate joined phase | 60 s | 240 s | keep permit; quarantine/fail session; never claim termination |
| synchronous work | capture through publish, excluding cleanup | 60 s | 120 s | reject up front if unfit; on expiry cancel then join cleanup before returning |
| task control request | authorized get/cancel/update registry operation | 2 s | 5 s | bounded infrastructure error; no job permit acquisition |
| non-delivered task | response never handed to transport send | 30 s | 30 s | cancel and join as orphan; retain visible cancelled record and request lease |
| task record | fixed lifetime from creation | 7,200,000 ms | 7,200,000 ms | remove result after expiry; active work was already bounded to 1 h |
| polling hint | client interval | 1,000 ms | 1,000 ms | advisory only; requests still face normal admission |
| terminal records | retained entry quota | 64 per owner | 256 per server | reject a new job before execution; never evict another owner early |
| retained task bytes | serialized results | 32 MiB per owner | 128 MiB per server | reject reservation or publish a bounded omission/failure, never partial pass |
| one MCP response | serialization/publication | 512 KiB | 512 KiB | explicit bounded omission and conservative status; no truncation-as-pass |

The phase arithmetic is composable: default children use 60 + 180 + 30 = 270 seconds
within the 300-second work parent, and maxima use 120 + 3,360 + 120 = 3,600 seconds.
Cleanup is separate and explicitly excluded from both async and synchronous work
budgets. A maximum synchronous request can therefore remain open for up to the
120-second work budget plus 240-second cleanup maximum: 360 seconds
worst-case return time. Security cleanup wins over the shorter availability target.

The two-hour fixed task lifetime counts the maximum 3,600-second work plus
240-second cleanup (and conservatively the 5-second seed boundary), leaving 3,355
seconds—more than 55 minutes—for terminal observation. It is distinct from ADR-061's
separate artifact TTL. `ttlMs` is never `null`, is not client-configurable,
and does not slide on poll/update. Task-record quotas are independent of ADR-061's
artifact quotas (64 MiB/job, 128 MiB/owner, 256 MiB/global, and 128
members/job); no alignment is inferred from the task entry counts.

The candidate-bound evidence is
`docs/validation/M3-02-budgets.json`, on the approved image
`sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`.
It contains every raw sample and phase measurement, not synthetic estimates:

| Qualified synchronous operation | Cold p50/p95/p99 | Warm p50/p95/p99 | Observed max |
|---|---:|---:|---:|
| nextest passing | 1,658 / 1,719 / 1,724 ms | 1,665 / 1,736 / 1,751 ms | 1,751 ms |
| coverage known-counts | 2,743 / 2,842 / 2,863 ms | 2,765 / 2,842 / 2,851 ms | 2,863 ms |
| SemVer identical | 1,697 / 1,747 / 1,752 ms | 1,698 / 1,739 / 1,742 ms | 1,752 ms |

Each row has 30 cold and 30 warm samples. The nextest samples were carried
forward from the current attempt log rather than produced by the recorded run,
as retained in the receipt; coverage and SemVer were measured in the final
assembly run. Mutation remains task-only, so it is not a
candidate synchronous operation. The live task probe measured a 262-byte
creation response, 1,048 resident job-record bytes, 0 ms observed poll latency,
1,088 ms from cancellation intent to joined cleanup, and 346 ms from EOF to joined
cleanup. It also observed cancellation in Admission, Execute, Publish and Cleanup,
and the uncertain-cleanup branch failed the session.

The measurements reaffirm every number in the decision table. The 60-second
synchronous default and 120-second maximum retain substantial serialization and
host-variance headroom above the 2,863 ms observed p99/max. The 2-second task-control
default covers the 0 ms poll and 1,088 ms cancel-to-cleanup observation without
making cleanup itself subject to that control deadline; the 5-second maximum remains
the tested fail-closed ceiling. The 5-second seed boundary, 30-second non-delivery
window, 300/3,600-second job budgets, 60/240-second cleanup budgets, fixed two-hour
TTL, quotas and 512 KiB response ceiling are safety/resource bounds rather than
latency predictions and are explicitly reaffirmed. Polling does not extend TTL.

### Restart, crash and observability

Jobs and task records are session-local and initially in-memory. Graceful EOF or
shutdown cancels all jobs and waits for verified gateway cleanup before exit. A new
process never resumes a job and every prior task ID returns the same masked
`task unavailable` response. Persisted artifacts may survive under their own
owner-bound authorization and TTL, but they are completed-result evidence, not a
checkpoint or permission to restart execution.

An abrupt process/host crash cannot run in-process join logic. OS/container
containment remains the safety boundary. **M3-01's integrator owns new work** to
extend the current lazy, volume-only `RustGateway::new` residual check to both
volumes and containers before the first M3 job is admitted. The default remains
fail-closed quarantine: a residual object is cleaned automatically only when its
fixed gateway label and unpredictable process-instance nonce prove it belongs to
this process. A label without the matching nonce, an unknown object, ambiguity or
inspection failure is quarantined and blocks M3 admission rather than deleting
another instance's evidence. This is not existing startup cleanup, and it must not
reconstruct `Running` from artifacts or a registry file. No daemon, broker,
privileged account or background collector is introduced.

Emit structured `tracing` events to stderr only for admission accepted/rejected,
job start, phase transition, cancellation intent, expiry, cleanup observed,
cleanup uncertainty, terminal publication, orphan cancellation, retention eviction,
and session shutdown join. Allowed fields are the opaque job ID, closed job kind,
phase/outcome/reason enums, elapsed/budget milliseconds, aggregate byte counts and
permit/quota counts. Never log source, paths, symbols, arguments, environment,
diagnostic bodies, task result bodies, project references, client-provided identity
or policy secrets. stdout remains protocol-only.

### Required discriminating tests

M3-01/M3-02 cannot mark this decision implemented without at least:

- `D06-T01`: the product protocol harness covers five versions × declared/not
  declared, always observes the decided advertisement, and creates/polls a task in
  every declared cell;
- `D06-T02`: every undeclared cell gets `-32021` with required capability from
  `tasks/*`; `execution_mode: task` gets fixed `-32602` with no data; a long or
  unqualified `auto` gets structured `isError`/`TASKS_REQUIRED` before admission,
  while a qualified short fallback stays within its work budget;
- `D06-T03`: cross-ProjectRef, cross-grant, malformed, expired and unknown IDs are
  indistinguishable and do not echo the ID or reveal owner/result/existence;
- `D06-T04`: ProjectRef revocation or policy-generation change during execution and
  before each get/cancel/update hides the task, cancels if needed and prevents result
  publication;
- `D06-T05`: cancel before start, during execution, during publication and during
  cleanup; `Cancelled` is never visible before observed joined cleanup;
- `D06-T06`: cancellation racing `CreateTaskResult` commit/suppression either rolls
  back before commit or uses request-ID delivery state after commit; successful send
  disables the short deadline, never-handed-to-send cancels/joins within 30 seconds,
  and its owner sees `cancelled` until expiry. Prove that rmcp's post-queue
  cancellation of `RequestContext::ct` does not reach the registry-owned job token;
- `D06-T07`: one active job makes a second call and worker-backed Resource read
  return existing busy/`SANDBOX_DENIED` without a queue or reservation, while
  get/cancel/update remain responsive without the worker permit. Exercise exact and
  one-byte-over retained task result limits at 32 MiB/owner and 128 MiB/server, plus
  64/256 terminal-entry saturation, and prove reject-before-execute/no eviction;
- `D06-T08`: stdio EOF with an observed hostile child cancels, terminates, drains and
  joins it before shutdown; uncertain cleanup fails the session and prevents reuse;
- `D06-T09`: expiry and every phase deadline terminate/join before permit release;
  include just-under/over the 5-second seed commit and induced just-under/over
  2-second default and 5-second maximum task-control deadlines. Poll does not extend
  TTL and `ttlMs` is always the fixed non-null value;
- `D06-T10`: restart exposes no prior running job, does not resume execution, masks
  old IDs, detects both residual volumes and containers, cleans only matching
  label+process nonce objects, and quarantines every ambiguous/foreign residual;
- `D06-T11`: ordinary `isError: true`, infrastructure failure, late success after
  cancel intent, and cancellation map to the decided terminal states without
  unknown/partial/unavailable/skip becoming pass. Serialize an exact 512 KiB response
  and a one-byte-over response, proving explicit omission/failure and no
  truncation-as-pass;
- `D06-T12`: `tasks/update` cannot grant authority or mutate job configuration and
  rejects input for authorized M3 jobs;
- `D06-T13`: 16 suppressed-response leases preserve ADR-030 behavior and the next
  request closes the session; task cleanup and job capacity remain independently
  correct;
- `D06-T14`: trace events contain the required bounded fields and no project/source/
  argument/result data; stdout contains only MCP frames;
- `D06-T15`: record candidate-bound G4 receipts for MCP Inspector 2.5.0 and
  model-driven Codex CLI 0.153.0 plus the five-version harness. If neither stock
  client declares Tasks, record that limitation, qualify Tasks only with the
  repository harness, and qualify synchronous fallback with both stock clients.

The checked-in rmcp-only wire spike is narrower than these product tests. It proves
SDK negotiation/serialization and cooperative cancellation without using the
product handler, product tools, processes, network or new dependencies.

## Alternatives considered

- **Wrap `rmcp::TaskManager`.** Rejected because its task ID lookup is not
  owner-bound, unknown-task errors echo the ID, `None` TTL is unlimited, expiry and
  shutdown abort futures without gateway cleanup proof, sweeping is opportunistic,
  and it cannot enforce ProjectRef/policy revalidation or admission permit lifetime.
- **Use rmcp TaskManager plus a parallel authority map.** Rejected because two
  registries create split-brain publication, expiry and cancellation races; the
  application lifecycle would still depend on SDK semantics.
- **Tasks-only tools.** Rejected because Tasks is an optional negotiated extension
  and bounded short operations can remain useful to clients without it.
- **Synchronous-only long execution.** Rejected because it ties result delivery to a
  long request, worsens cancellation/retained-slot behavior and cannot satisfy the
  bounded fallback rule.
- **A custom polling tool or JSON-RPC stack.** Rejected because rmcp already owns
  MCP dispatch/negotiation, global enumeration would create an authority leak, and a
  second protocol stack violates the architecture.
- **A queued or multi-job executor.** Rejected until measured resource and cleanup
  evidence justifies a new admission ADR. Initial policy is one active job/no queue.
- **`ttlMs: null`, sliding TTL, or client-selected retention.** Rejected as
  unbounded or attacker-renewable retention.
- **Persist jobs and resume after restart.** Rejected because process state,
  authority and hostile-code containment cannot be reconstructed safely from a
  record. Only completed private results may persist under a separate decision.
- **Daemon, broker, privileged account, or collector.** Rejected by ADR-050's local
  coordinated deployment boundary and because it would add installation and trust
  requirements without solving host-authority revalidation.

## Consequences

The application receives one neutral lifecycle for synchronous and negotiated-task
execution, while rmcp remains the only MCP implementation. The product can preserve
one-job admission, joined cleanup and host authority across polling and cancellation.
Task IDs and terminal results remain bounded, private and non-enumerable.

The cost is a product registry/watchdog and explicit MCP mapping instead of the SDK's
convenience manager. Polling is required; 3.2.0 offers no routed task-subscription
path used here. A job monopolizes the existing worker permit, so other tools and
worker-backed M1 Resource reads return busy until joined completion. Task registry
control and non-blocking quality Resource reads remain responsive. Task creation has a bounded non-delivery orphan window
because response delivery is not transactional. A peer without Tasks cannot run a
long M3 operation.

The production server advertises Tasks after the five-version product matrix and
the candidate-bound Inspector/Codex gate passed. This decision does not add
`rust.dependencies.inspect`, make task execution durable, weaken the existing 18
M1/M2 tool contracts, or make a peer declaration optional.

## Sources

- `docs/roadmap/m3-quality.md`, especially “Arquitectura, tareas y Resources”.
- `docs/roadmap/adr-backlog-m2-m8.md`, D06.
- `docs/adr/ADR-030-m1-worker-admission.md`.
- `docs/adr/ADR-032-project-inspection.md`.
- `docs/adr/ADR-037-test-execution.md`.
- `docs/adr/ADR-040-single-capture-quality-gate.md`.
- `docs/adr/ADR-050-local-coordinated-mutation.md`.
- `docs/adr/ADR-061-private-quality-artifact-store.md`.
- `docs/security-model.md` and `docs/architecture.md`.
- `docs/validation/m3-delegation/V06-adr060-review/last-message.md` and the
  orchestrator's A06b dispositions.
- Existing transport/worker/gateway sources:
  `crates/mcp-server/src/stdio/admission.rs`,
  `crates/mcp-server/src/stdio/workers.rs`,
  `crates/mcp-server/src/stdio.rs`, and
  `crates/execution-adapter/src/rust_gateway.rs`.
- Pinned `rmcp 3.2.0` source:
  `src/model/capabilities.rs:197-204,245-252,429-465`,
  `src/handler/server.rs:22-48,203-243,318-354`,
  `src/model/task.rs:1-69,156-215,272-386`,
  `src/task_manager.rs:280-298,309-398,401-471,485-537`,
  `src/service.rs:1248-1255,1483-1487`, `src/service/server.rs:238-247`, and
  `src/model.rs:159-175,4332-4384,4596-4609`.
- `docs/validation/m3-delegation/R00-rmcp-tasks/report.md`, independently
  corroborated against the pinned source above.
- `crates/mcp-server/tests/rmcp_tasks_spike.rs`, the offline five-version wire
  observation accompanying this decision.
