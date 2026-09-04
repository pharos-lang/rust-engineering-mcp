# M1-16 independent controller review

Task: bounded read-only review of broker.py/participant.py against ADR046 and protocol v2, 2026-09-04. No model invocation, Docker, project execution or code changes. This review concerns broker/participant, not an independent review of my own Rust driver/emitter implementation.

Result: four concrete findings requiring disposition before freeze. No source-grounded filesystem escape or hidden-oracle leak was found in the admitted broker surface. That is a bounded source-review conclusion, not proof against V8 vulnerabilities or hostile same-user OS mutation.

## Findings

### P1 — App-server pipe backpressure can prevent cancellation and completion

`target/m1-16-controller/participant.py:119-123` writes/flushes up to 1 MiB through a blocking BufferedWriter while holding `self.lock`. `interrupt` at 209-213 acquires the same lock through `send`, and finalization at 326 joins the watchdog without a bound before transport cleanup. If app-server stops reading its stdin, a source/tool reply larger than pipe capacity can block the caller inside write; watchdog cannot acquire the lock to send cancellation. If the watchdog itself blocks in its write, finalization blocks joining it. The cancellation event alone cannot release these writes, so the expected bounded turn interruption and eventual owned-process cleanup never start. This is an infrastructure deadlock path, not the documented allowance for cooperative gateway cleanup beyond 900 seconds.

Recommended discriminator: a trusted dummy child that retains but never drains stdin; send an allowed large message, trigger deadline, verify admission stops and cleanup exits within a bounded transport interval with explicit failure, without treating forced cleanup as success. Use nonblocking bounded writes with deadline/cancellation handling and a cleanup path independent of the blocked writer/lock. No reproduction child was run in this read-only review.

### P1 — Broker reports server cleanup from an execution-only acknowledgement

`target/m1-16-controller/broker.py:192-199` computes joined from `execution_joined` and process exit code, then assigns `server_cleanup_verified: joined`. It never examines `server_joined`. The current driver explicitly distinguishes these: raw has no server and reports `server_joined:false`; MCP can finish the Rust execution loop yet fail its child wait. Both receipts must not be promoted to server-cleanup verification by an execution-only ACK. Exit1 is accepted during cancellation, so that cannot distinguish these cases. This violates ADR046's separate SDK server/gateway join condition and hides uncertainty from the runner.

Recommended discriminator: canceled MCP exit1 plus `{execution_joined:true,server_joined:false}` must not yield server cleanup verified or successful MCP cleanup. Test raw no-server semantics separately. Preserve both receipt fields; successful process join is still not independent proof of Docker container/scratch cleanup.

### P2 — Malformed arguments to an admitted tool terminate the run instead of a retryable denial

`target/m1-16-controller/participant.py:277-284` combines allowed tool identity with root-property/required-field validation into `admitted`. Extra or missing fields therefore reach `interrupt()` before the broker can return its normal retryable denial (`broker.py:510-522`). Protocol v2:208-209 explicitly says invalid requests receive retryable denials. A participant typo in an otherwise authorized operation is currently scored as an interrupted run; the model cannot repair its call. This differs from nested typed MCP errors and broker value errors, which are returned normally.

Recommended discriminator: an admitted tool with an extra/missing field, followed by a corrected call in the same session, should log both and permit the second without granting extra authority. Continue failing closed/interrupting unknown server methods or tool identities. Count denied attempts consistently toward the fixed admission limit so retryability cannot create an unlimited loop.

### P2 — Mandatory freeze does not bind the executable providing the reviewed confinement

`participant.py:20,68,188` launches the current `/opt/homebrew/bin/codex`; there is no version/hash verification there. Supporting runner `run-study.py:124-138` mandates script, driver/server, corpus and asset hashes but not Codex or its resolved code-mode-host binary. ADR046's confinement depends on the inspected rust-v0.153.0 native tool-registration and narrow V8 implementation. A normal installed CLI update could therefore change that implementation while the current mandatory freeze checks still pass. Equal feature-flag values do not establish identical callback/native-tool behavior.

Recommended discriminator: freeze expected Codex and resolved code-mode-host real executable identities/hashes, reject their drift before a turn, and require those files rather than merely allowing an owner to add optional freeze entries. Record which reviewed source receipt supports those binaries. No installation/update or executable inspection was performed by this review.

## Boundary observations and limits

- Workspace paths are a closed three-file set; reads check no-follow, regular file, single link, ownership, permissions, identity and immutable manifests (`broker.py:233-250,308-364`). Candidate artifacts are siblings outside the one visible project root, written through owned directory handles (`366-381`). Hidden/reference paths never enter this public closure.
- Host init is outside dynamic callbacks. Broker checks arm/root consistency at 396-406 and requires explicit trusted `allow_project_code` for all validation operations at 453-467. The runner supplies it explicitly at 192-194. This is host consent, not a new model-granted execution permission.
- Driver is spawned under an empty environment and explicit IP-outbound denial (`broker.py:35-36,70-72`). Participant app-server receives only HOME/TMPDIR/LANG/CODEX_HOME plus fixed PATH (`participant.py:58-60`), uses supported host authentication and deliberately is not an air-gapped process. No arbitrary secrets or full environment are copied into the participant transport.
- Effective guards, configured MCP disables, empty instruction/roots, exact model/provider/effort are checked before the turn (`participant.py:175-190,238-260`). Config parsing may fail to disable unusually quoted MCP table names, but the subsequent effective all-disabled check rejects the run; this is fail-closed, not a demonstrated inherited-MCP bypass.
- The V8 confinement statement is explicitly language-runtime/source based. `target/m1-16-smoke/code-host-proposal.md:15-45` identifies globals/import/callback and registration boundaries. `target/m1-16-smoke/README.md:59-64` correctly labels the reported canary globals/inventory as model-reported, with no independently retained raw cell attestation. The timeout receipt proves one observed interruption/cleanup path, not stalled stdin, complete tool inventory or a universal OS sandbox. I did not rerun those checks or infer stronger guarantees.
- Both arms share workspace reads/submissions and candidate caps. Validations, metadata, explain and strict/test limits are separately admitted. The raw quality response exposes fixed command evidence plus explicitly qualified std-only lock facts; B exposes product structured evidence. Those represent the declared treatment, not a discovered hidden-oracle disclosure. Source semantic constraints remain a separate post-run review obligation; the broker does not establish no-unsafe/no-tests/no-hardcoding by checking filenames.

Files changed: only this review artifact.

Tests executed: none; read-only inspection and SHA-256 capture only. Existing tests/receipts were inspected, not represented as newly executed evidence.

Reviewed source digests:

- broker.py: `1b0df264ace2eed6b25b93a4e9e66fafa9464ee944b735a6c532dd4ef4d9b372`
- participant.py: `6163760827084da4aa6f622dd82db86e38bde4a0fd47b4c6089bd882c24cf74f`
- run-study.py: `d974fc8a1a70664c52dd4a65944535a52817ef01bb7c6ca6bfbd9030ba3e199d`

Risks: no adversarial V8 testing, actual transport backpressure test, actual gateway cancellation or model run was authorized for this review. Source may change concurrently; findings refer to the hashes above.

Decisions: no architecture/protocol/code change made; principal owns dispositions and external independent review.

Open issues: resolve or explicitly disposition the four findings, rerun discriminating qualification, then freeze. This report does not close M1-16/M1 or establish experimental efficacy.
