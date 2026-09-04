# M1-10 principal review disposition

2026-09-04, branch `ai/m1-10-catalog-import`, before integration. Independent
Claude Code2.1.259 reviews used explicit `claude-opus-5`, high effort, tools disabled,
restricted/safe mode and no MCP servers. Receipts retain packet/result hashes and
reported actual model usage. CLI auxiliary Haiku usage is separately visible;
the substantive reviews report Opus5. Reviewers inspected supplied code, not tests
executed independently. Principal tests/gates provide execution evidence.

## Import/storage first packet

[Review](M1-10-import-review.md), [receipt](M1-10-import-review-receipt.json).

| Finding | Principal disposition |
| --- | --- |
| P1 trust permissions | Accepted. Dedicated owned0600 trust read, owned0700 parent, protected root/current-owner ancestor handles rechecked, narrow sticky `/private/tmp` exception. POSIX modes do not inspect ACL grants; host provisioning must exclude third-party write grants. Real permissions/link/ancestor cases and ownership predicate tested. |
| P1 damaged active prevents recovery; P2 key rotation | Accepted. ADR-041 revised before implementing independent checksummed floor bound to publisher/channel/sequence/exact container. Reserve-before-activate, exact reserved retry, higher signed sequence and invalid/missing floor fail closed. CLI tests cover restart, corruption, missing active/floor, pending reservation and rollback. |
| P2 import peak memory | Accepted. Extract active sequence and drop parsed active generation before acquiring candidate; drop active compressed bytes earlier. Byte limits remain distinct from aggregate RSS. |
| P2 container malleability | Documented intentional container identity. Catalog identity is publisher/channel/sequence/SQLite hash; reserved retry requires exact compressed bytes. No authenticity claim for compression metadata. |
| P2 semantic availability/unsigned index | First packet was incomplete. Final rebuild reopens actual objects; import validates native objects/model/catalog/name membership before activation; status validates persisted data before true. Unsigned local rebuild is trusted owned host state, not publisher distribution. See native review below. |
| P2 same catalog/index directory | Safe rejection retained through exclusive flock, including aliases; documented distinct directories. CATALOG_BUSY is less specific but no overwrite is possible. No string-only alias check introduced. |
| P2 UNIQUE/beneath | O_UNIQUE named against XNU and tested with actual hardlinks; independent fstat single-link remains. Comments distinguish root-anchored path resolution from fixed-name handle confinement. |
| P2 status takes write lease | Explicit CLI behavior documented: lock and discard staging. Future MCP runtime must use read-only acquisition and never hold this admin lease. |
| P2 read/commit inode window | Trusted owner must preserve state; cross-process cooperating admins share flock. Same-UID hostile state replacement is outside guarantee. Fixed staging identity, root/lock stamps and readback remain enforced. |
| P2 minor IO/budget issues | Changed now maps CATALOG_STATE_CHANGED. APFS-only input is explicit compatibility restriction. RustSec retains existing parser bounds plus global bundle cap/deadline. Buffer reservation now amortized. |
| Test gaps | Added signed multi-payload RustSec/semantic metadata matrix, domain separator, signed publisher/channel mismatch, sequence overflow; real E5 embedded index import and corrupted-native rejection. Fault tests are checkpoint injection, not power-loss tests. Native non-macOS qualification remains pending. |

## Native persistence and HTTPS packet

[Review](M1-10-native-network-review.md), [receipt](M1-10-native-network-review-receipt.json).

| Finding | Principal disposition |
| --- | --- |
| P1 unsigned external index | Accepted exposed shared-path concern. External derived reads now use the protected owner0600/parent0700/ancestor reader before native decoding. Locally rebuilt state is explicitly trusted owner state; checksum is corruption detection, not authentication against malicious same-UID/admin rewrites. Publisher imports remain signed. Host ACL provisioning requirement applies to both. |
| P1 row count before inference | Claimed oversized path was not reachable: catalog records::all checks count<=1000 before allocation; embedding_documents calls it. Added local <=1000 check before vector allocation/inference as defense in depth. |
| P2 E5 arity/cast | Model input array now uses E5_FILES.len(), with verifier fixed array enforcing compile-time agreement. Dimension cast changed to checked conversion. |
| P2 index section/version chain | Native parser handles its format; only memory provider exists and searches bypass secondary vector indexes. Imported object references cannot acquire external stores. This cut exports fresh full tables, no ANN guarantee. Any future index use needs explicit validation/ADR. |
| P2 JSON identity determinism | Deliberate byte-level versioned identity; all current fields ordered structs/scalars, no map. Metadata schema changes must version/rebuild. Fail-closed mismatch, not incorrect facts. |
| P2 session cache units | Checked pinned lance8 Session::new and lance-core8 Moka cache: weighted memory capacity, not entry count. Limits still do not bound all native allocations. |
| P2 registry test depth | Exact memory-only registry and no external providers; native malicious manifest tests plus real E5/Lance runtime under OS network deny. No claim that a unit registry assertion alone proves containment. |
| P2 existing root/Docker CLI parsing | Existing downstream SecureProjects/gateway path validation rejects relative/nonphysical paths. Unchanged M1-01..09 boundaries; no demonstrated regression. |
| P2 linked TLS capability | Runtime never calls catalog_sync; main dispatch selects separate catalog CLI branch. Compile-time absence is not required. Explicit host HTTPS acquisition remains separate from tools. |
| P2 identity case/TLS version | Strict lowercase identity encoding is deliberate fail-closed compatibility. Pinned rustls defaults support TLS1.2/1.3. No insecure fallback. |
| P2 nested runtimes | Helpers run from synchronous CLI; future runtime must invoke expensive work from joined blocking workers. No call from Tokio reactor in current dispatch. |
| P2 concurrent native generations/vector meaning/model read | Aggregate peak memory documented. SQLite facts authoritative; publisher/host-derived vectors trusted as ranking data, not proven semantic relevance. Model identity validation currently loads verified model; startup/quality measurement remains M1-16. |

A focused internal read-only native review also caught cooperative deadline checks
missing after the last inference/native phase. Fixed checks include model loading,
after each inference and native build/export/restore; late native returns cannot
report successful rebuild. This is not forcible cancellation of native calls.

## Floor follow-up

[Opus5 follow-up](M1-10-floor-review.md), [receipt](M1-10-floor-review-receipt.json).

- P1-1 preconditions are impossible through private VerifiedBundle: verify enforces
  sequence1..i64::MAX, trusted publisher/channel and computed lowercase hash.
  Added fresh floor serialize/parse validation before durable reservation anyway.
- P1-2 accepted: status now exposes floor sequence/hash and pending reservation in
  JSON and human output; interrupted-reservation test asserts those fields.
- P1-3/4 accepted operational clarity: distinct CATALOG_ACTIVE_UNVERIFIED and
  CATALOG_TRUST_MISMATCH; invalid floor remediation never suggests resetting it.
  Actual Ed25519 key42→key43 test imports a newer signed generation without losing
  floor; channel change leaves floor bytes intact. Modified valid-JSON floor with
  stale checksum fails closed.
- P2 lock structural corruption retains precise layer error (oversize/budget versus
  link/permission denial); no reset or ignored corruption. Store is deliberately
  mechanical, CLI validates monotonicity before every reservation. Future read-only
  runtime receives no write lease. Aliased index/store still fails closed at flock.
- P2 double index load removed: configured external index takes precedence and the
  embedded validator is skipped. CLI status write-lease behavior remains documented.

Full15/15 passed on immutable pre-observability source; final CLI changes receive
fresh core, all-features Clippy and real native CLI gates. Evidence explicitly
retains both source sets rather than calling the earlier full a later-code run.
Final focused Sonnet5 review and local integration are recorded in M1-10.md.


[Sonnet5 Medium final follow-up](M1-10-observability-review.md) found no correctness
or security defect in final CLI/error/recovery logic. Its medium test-coverage
finding is fixed: the pending-reservation scenario now runs the real binary without
`--json`, checking active/floor/pending and offline semantics on human stdout with
empty stderr. Low notes require no change: fixed error coalescing is intentional;
serde deny_unknown_fields on serialize-only CLI types is not asserted to enforce
an output schema (MCP contracts use their independent schema validator).
