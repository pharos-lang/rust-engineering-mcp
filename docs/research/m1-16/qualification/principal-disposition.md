# Principal disposition of pre-freeze Opus review

2026-09-04. Review received an older packet; every finding is assessed against the
current research controller and production source. No utility run or freeze yet.
Pending real requalification is a gate, not a passed finding. Source paths below
refer to reproduction/controller or reproduction/driver, with executable originals
under target/m1-16-* until the approved preservation step.

| Finding | Principal disposition and evidence |
| --- | --- |
| C1 | Accepted underlying bug: cancellation could mask cleanup errors and the broker accepted exit1 too broadly. Driver now preserves prior errors, carries sticky cleanup_uncertain, and broker requires explicit false, execution join, server join for MCP, exact cancelled terminal error for expected exit1. Actual resumed raw cancellation passed; MCP initially failed conservatively. Investigation found SDK cancellation closes its stdout reader before server shutdown drains late replies. Retaining a duplicate read endpoint and draining after SDK shutdown restores actual child exit0; focused native qualification passed. No whitelist of arbitrary child failures. |
| C2 | Reject Drop premise. Production crates/execution-adapter/src/rust_gateway.rs:852 calls synchronous cleanup before finish_work. cleanup_inner at584–642 removes and checks containers/volume. Still accepted that join alone is insufficient: separate certainty flag and independent read-only Docker absence checks now run before/after every participant and repair oracle. |
| C3 | Accepted/fixed. Broker explicitly requires server_joined in MCP mode; missing/false and uncertainty have discriminating tests. Separate execution join and cleanup_verified evidence. |
| H1 | Accepted defense in depth: app-server cwd now fresh0700 /private/tmp/m1-16-neutral-*, outside repository/corpus/evidence. Only empty joined cwd is removed; nonempty/uncertain preserved. The no-environment source-reviewed API remains the actual access boundary; cwd location alone is not a sandbox. |
| H2 | Fixed before this disposition: admitted tool with object arguments reaches broker required/extra/domain validation and retryable denial. Unknown/unadmitted native tool requests fail closed. B has17 dynamic declarations:13 mapped tools,3 common submissions/reads,1 Resource callback. |
| H3 | Accepted exact config/binary guard requirement, not an invented full tool-inventory attestation. Runner pins actual CLI/code host, driver/product and frozen assets before/after runs. First resumed exact-feature check exposed that historical preflight had filtered its map; it stopped before any model turn. Source-grounded real config/read now passes41keys (38false,2true,network_proxy null); no model turn in that preflight. Corrected exact-model echo completed with joined cleanup. Empty environments/instructionSources and reviewed native registration are required; harmless clock/plan/input metadata may remain identically. |
| H4 | Accepted as a declared shared censoring risk, not proof quality is unusable.512KiB encoded callback cap,1MiB driver line,16MiB cumulative IPC and logs; raw256KiB/stream/30s. Current20 oracle/42 SDK calls fit. Preserve overflow as infrastructure failure and retain outcomes; do not truncate an arbitrary MCP envelope into misleading evidence. Actual quality compound calibration included. Limits do not imply equal information content; representation is the treatment. |
| H5 | Shared prompt already discloses6 candidates/validations,64calls,900s/30k tokens, strict Clippy and test<=30. B additionally receives its host-authorized root. Prompt and exact ordered tool-schema hashes recorded. Native product descriptions unchanged to measure actual interface. |
| H6 | Fixed runner verifies approved hash freeze, config, executable/controller sources, CLI/codehost/product/driver, corpus/projection and all configured assets before/after each run. Exact copies retained under docs/research; source commit and receipts join freeze. No cargo clean authorized. |
| H7 | Reject conflation with Rust semantic parsing. Arm A's clearly labelled immutable Cargo TOML/lock fact is limited to one workspace package and zero third-party locked packages. It is not a vulnerability/security scan; B has actual snapshot audit. Independent final audit is identical for both arms. No general safe/dependency-clean claim follows. |
| H8 | Reject live-host Cargo premise. Production project.open is structural; check/test capture SourceBundle and run approved RustGateway. A and B create fresh per-execution Docker source/target volumes and frozen locks. No host compilation or cross-validation Cargo build cache. |
| M1 | Returned handler result retained with cancellation_after_handler; committed broker candidate remains authoritative even if response delivery fails. Errors and known cooperative cancellation classified separately. |
| M2 | Global submission indices include both kinds. Runner records artifact_path from patch ordinal while preserving candidate ordinal; evaluator treats a wrong-kind first candidate as failure, never skips it. Six global submissions enforced. |
| M3 | Accepted/fixed: cleanup errors cannot escape receipt finalization; process monitor is locked, stopped/joined before snapshot. Unknown inspection is null/false, not absent. Tests cover actual pipes/owned child and injected ps errors. |
| M4 | Accepted/fixed: fresh run mkdir and participant events/receipt/neutral-marker exclusive guard. No automatic replacement/resume. |
| M5 | Study runner hardcodes authorized allow_project_code=true and frozen driver config. Reusable broker's explicit false denial is not reachable study misconfiguration; never infer consent. |
| M6 | Accepted/fixed: byte-bounded FIFO16MiB replaces queue32/.2s; actual100-message pipe backlog test. |
| M7 | Reject proposed additional security boundary: private driver IPC is host-only, not model tool surface. Broker fixed three-file handles are the admission boundary; hidden harness injected only by evaluator. Sharing trusted driver binary does not grant a callback to arbitrary IPC/files. |
| M8 | Accepted limitation: fixed counterbalance and actual within-pair order/prior-run IDs; record cached tokens. Provider caches and scheduling can confound timing. |
| M9 | Fixed: emitted baseline projection has actual snapshot_fingerprint/source_id; runner validates both before model. Old staging projection remains historical and is not used. |
| M10 | Effective config mcp_servers all false is the enforcement. Regex merely generates overrides; omitted server fails config check. No claim that scanning one TOML file is authoritative. |
| M11 | Accepted/fixed: nonblocking bounded writes and lock, bounded watchdog join, real pipe tests and actual interrupted-turn qualification. |
| M12 | Same16MiB cumulative cap both arms; rich representations may consume it differently. Treat censoring separately and report, never silently exclude failed runs. Not equal-information-budget claim. |
| M13 | Reject stated immediate-timeout premise: participant starts30s post-cancel receive deadline only after synchronous handler returns. Gateway join can extend wall admission; elapsed/cleanup separately retained. Uncooperative handler cannot be safely detached. |
| M14 | Verified official pinned core/src/session/session.rs:846–848 skips thread persistence and933–941 skips local state DB when ephemeral. Source URL/hash preserved in persistence-source-receipt.json. This does not claim absence of host service/auth/usage metadata or provider retention. No cross-run model history is supplied. |
| L1 | Orthogonal task_status, infrastructure_failed, cleanup_failed preserve original turn outcome. Runner stops series on participant cleanup uncertainty or input drift. |
| L2 | Actual observed_requests retained alongside capped admission count. |
| L3 | Explicit first stop_reason and stop_reasons. |
| L4 | OutputTokens includes reasoningOutputTokens; do not add the subset twice. Observed threshold only, partial/unknown usage preserved. |
| L5 | Sanitized RPC preflight event sequence and effective configuration hashes retained; no raw config/auth bodies. |
| L6 | Mid-commit failure is infrastructure failure and preserves workspace/orphan evidence. No successful candidate is invented; fresh-run prohibition prevents replay collision being hidden. |
| L7 | Retryable broker_error now delivered success:false without interrupt. This transport field is separate from task success. |
| L8 | Existing real timeout qualification exercised string interrupt ID. Requalify final participant changes before freeze. |
| L9 | Reversible dot-to-underscore mapping explicitly recorded; exact MCP schemas/results retained. Resource callback plus3shared controller operations are not extra public M1 tools. |
| L10 | No cross-run Cargo target cache; Docker image/OS page/provider caches remain warm and are timing confounders. |

The claim that no dynamic roundtrip had occurred was a packet omission: retained
smoke-host and postreview echo receipts contain actual admitted exact-model calls.
Those are infrastructure qualifications, never utility evidence. Final changed
controller requires a fresh disjoint echo. Final Opus follow-up must review current
files, this disposition and actual requalification before principal freeze.


## Final Opus follow-up disposition

[Actual Opus5 medium review](opus-followup.md), tools disabled, reports no P0 and
permits freeze conditional on three P1 dispositions and requalification. Actual
modelUsage in the receipt confirms Opus5; CLI auxiliary Haiku is separate.

- P1-1: retained fail-closed unknown/non-object callbacks, explicitly amended
  protocol and shared prompts. Only admitted-tool object field errors are promised
  retryable. Extra callback-name attrition is an acknowledged comparison limitation.
- P1-2: implemented real SDK catalog.status before EACH arm's model window; compare
  complete frozen model identity/index metadata/documents/snapshot. A temporary
  setup session is joined; B retains its session. Actual check passes in
  catalog-setup.json; unavailable/wrong identity tests fail. Config missing keys
  reject explicitly. No silently lexical pre-run treatment.
- P1-3: evaluator derives corpus date from frozen provenance observed_at in UTC;
  test changes epoch and rejects absent/bool timestamps. No hardcoded scoring date.
- P2-1: missing selection labels reject before evaluation mkdir.
- P2-2: removed raw close of borrowed buffered-stream descriptors entirely. Alive
  reader owns its stream and fails cleanup; verified joins permit ordinary close.
  Real blocked-pipe test proves bounded return and retained FD ownership.
- P2-3: explicitly accept strict observer stderr policy; even warnings fail safe.
- P2-4: actual positive observer during owned Cargo execution now records nonempty
  container AND volume lists, followed by empty post-cancellation lists in both
  arms. This exposed an invalid volume .ID format hidden by empty output. Changed
  volume formatting to .Name per [official Docker volume ls documentation](https://docs.docker.com/reference/cli/docker/volume/ls/#format-the-output---format). Earlier
  observation failed safe, not a false absence. Qualification error cleanup now
  signals/joins its request worker before touching Driver, avoiding concurrent
  readers; broker also tolerates advisory readiness/EAGAIN. No utility run affected.

The failed positive-control attempt is retained at
 target/m1-16-driver-qualification/run-1788543554942491000; its outer diagnostic
reported observer_command_failed and then competing-reader BlockingIOError.
Read-only post-incident Docker queries and process observation found no owned
containers, volumes or driver/product processes. No forced cleanup was needed.

Final qualification:33 participant,26 broker,15 evaluator,8 analyzer,13 driver and
1 bundle tests pass; fresh exact-model echo, actual catalog setup, both active
cancellations with positive/negative Docker observations and IPv4/IPv6 controls
pass. Full model/read/edit/quality qualification remains valid for its exact
prior controller hash; subsequent changes are the bounded fixes listed above.
Reproduction copies and freeze include current sources. Principal finds no open
P0/P1 blocking this explicitly limited private pilot. Release/platform/owner
prerequisites remain separate and are not waived.
