## Task

Package W4 is complete. M3-02’s negotiated Tasks path is qualified end to end, measured, documented, and advertised.

## Result

- Production `TASKS_ADVERTISEMENT_READY`: **enabled**
- Five-version declared/undeclared protocol matrix: **passed**
- Live Docker Tasks lifecycle: **4/4**
- Complete M3 runtime gate: **59/59**
  - nextest 19/19
  - Tasks 4/4
  - coverage 8/8
  - SemVer 18/18
  - mutation 10/10
- Rust security gate: **20/20**
- Workspace tests: **1,066 passed, 0 failed, 115 intentionally ignored**
- Inspector 2.5.0: declared Tasks and passed the complete lifecycle
- Codex app-server 0.153.0: did not declare Tasks; passed its synchronous/model-directed path
- Docker residue: **zero owned containers and zero owned volumes**
- No containment or ADR-030 semantics were weakened.

## D06 matrix

| Oracle | Principal evidence | Status |
|---|---|---|
| T01 | Five-version switch/declaration product matrix and protocol test | Passed |
| T02 | `-32021`, explicit-task `-32602`, `TASKS_REQUIRED`, sync fallback tests | Passed |
| T03 | Byte-identical masking of malformed/unknown/foreign/expired/revoked IDs | Passed |
| T04 | `tasks_revocation_during_active_child_masks_cancels_and_prevents_publication` | Passed, Docker |
| T05 | `tasks_cancel_before_start_during_execution_publication_and_cleanup_waits_for_join` | Passed, Docker |
| T06 | Pre/post-commit cancellation and suppressed-response lease regressions | Passed |
| T07 | One permit/no queue, responsive controls, Resource busy, retention saturation | Passed |
| T08 | `tasks_eof_joins_hostile_child_and_uncertain_cleanup_fails_session` | Passed, Docker |
| T09 | Phase deadlines, seed/control bounds, fixed non-sliding TTL | Passed |
| T10 | `tasks_restart_masks_old_ids_reconciles_objects_and_admits_fresh_work` | Passed, Docker |
| T11 | Terminal mappings and exact 512-KiB/one-byte-over behavior | Passed |
| T12 | Authorized update remains fixed and non-mutating | Passed |
| T13 | ADR-030 suppressed-response lease behavior remains intact | Passed |
| T14 | Bounded trace fields and stdout-only MCP frames | Passed |

The complete mapping is in [M3-02.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-02.md).

## Measured budgets

Each synchronous candidate used 30 cold and 30 warm samples.

| Operation | Cold p50 / p95 / p99 | Warm p50 / p95 / p99 |
|---|---:|---:|
| nextest passing | 1,658 / 1,719 / 1,724 ms | 1,665 / 1,736 / 1,751 ms |
| coverage known-counts | 2,743 / 2,842 / 2,863 ms | 2,765 / 2,842 / 2,851 ms |
| SemVer identical | 1,697 / 1,747 / 1,752 ms | 1,698 / 1,739 / 1,742 ms |
| mutation | Task-only | Task-only |

Additional observations:

- Create-task response: 262 bytes
- Resident job record: 1,048 bytes
- Poll latency observed: 0 ms
- Cancellation intent → joined cleanup: 1,088 ms
- EOF → joined cleanup: 346 ms

ADR-060 now explicitly reaffirms the 60/120-second synchronous, 300/3,600-second job, 60/240-second cleanup, 2/5-second control, 30-second non-delivery and fixed 7,200,000-ms TTL decisions. Evidence: [M3-02-budgets.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-02-budgets.json).

The receipt transparently records that nextest’s 504.830-second measurement log was resumed; final assembly took 1,224.557 seconds, while summed measured operations took 1,729.383 seconds.

## Client matrix

| Client | Version | Declares Tasks | Discovery | Positive | Failure | Cancel | Resource | Task flow |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| MCP Inspector | 2.5.0 | Yes | Pass, 22 tools | Pass | Pass | Pass | Pass | Pass |
| Codex app-server | 0.153.0 / GPT-5.6 Sol medium | No | Pass, 22 tools | Pass | Pass | Unsupported | Pass | Not negotiated; synchronous flow passed |

Final production run: attempt 12. Attempts 1–10 remain preserved as failures; attempt 11 preserves the pre-advertisement qualification. Receipt: [M3-02-clients.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-02-clients.json).

## Tests executed

| Command | Exit / counts | Duration / receipt |
|---|---|---|
| `scripts/test-m3-runtime.py` | 0; 59/59 | 1,231.454 s; [M3-runtime.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-runtime.json) |
| `scripts/test-rust-execution.py` | 0; 20/20 | 536.797 s; [M3-rust-security.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-rust-security.json) |
| `scripts/test-m3-budgets.py --run --samples 30 --resume-current-logs` | 0; 180 sync samples plus lifecycle probes | [receipt](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-02-budgets.json) |
| `scripts/test-m3-clients.py --run` | 0; Inspector and Codex | Attempt 12; [receipt](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-02-clients.json) |
| `cargo test -p rust-engineering-mcp --test protocol --locked --offline` | 0; 43/43 | 11.92 s |
| `cargo fmt --check` | 0 | 0.8 s |
| `cargo check --workspace --all-targets --locked --offline` | 0 | 3.46 s final run |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 0 | 4.07 s final run |
| `cargo test --workspace --locked --offline` | 0; 1,066 passed, 115 ignored | ≈60 s counted run |
| `python3 -B scripts/check-architecture.py` | 0 | 0.2 s |

All `catalog_sync` tests passed. The earlier failures were confirmed as a sandbox artifact.

## Files changed and SHA-256

The full 549-entry manifest—including sources, documents, receipts and immutable client attempts—is [W4-files.sha256](/Users/cburgosro/Projects/rust-mcp/docs/validation/W4-files.sha256), SHA-256:

`475d8ab6a570aa0fbc6c434ace5d0adb712f54072cbfa42b95ec5b979d0d8654`

Key receipts:

- M3 runtime: `9647a87f29d55d40f8fb78eeb67a9a33ef333cbeb57faba234d265c217e4bec6`
- Rust security: `581cc8f93f5bca16d2bb7946d41447bfb34658f516f58fed9cadecbb031fe451`
- Budgets: `4327ab761487d1c2b342fbd0ef9adbbcc952e863d771a103bb9409f981b5c864`
- Clients: `f48b1b68848e5803b3043db69032b8e1d27399d5a5e702c983b8e646195daff2`

Advertisement deliberately changed three tool-definition snapshots:

| Snapshot | Before | After |
|---|---|---|
| nextest | `93e79e…a0679` | `c05d55…09f3` |
| SemVer | `465b5e…981f` | `1c8df1…db5` |
| mutation | `08f377…080a` | `d68904…07d` |

The other 20 snapshots stayed byte-identical.

## Docker hygiene

After the budget, client, complete runtime and security gates, both commands returned empty output:

- `docker ps -aq --filter label=org.rust-mcp.execution=true`
- `docker volume ls -q --filter label=org.rust-mcp.execution=true`

## Decisions and risks

- Advertisement was enabled because Inspector declared Tasks and passed create/poll/cancel end to end.
- Codex app-server currently does not declare Tasks; long M3 operations therefore return `TASKS_REQUIRED`, while qualified short operations use synchronous fallback.
- Inspector required modern `tools/list` responses to include `ttlMs: 0` and `cacheScope: private`; the five-version protocol suite pins this compatibility behavior.
- Measurements are specific to this host, approved image and fixtures. The retained limits intentionally include substantial safety/headroom margins.
- The evidence-first sprint workflow was used to preserve every failed attempt and rerun receipts after the final contract-byte change.

## Open issues

M3-02 itself has no unexplained pending oracle. M3-06 and milestone-level G5–G9 handoff/release work remain open.

The protected documentation worker’s five files must ultimately state:

- 22 tools are present.
- M3-02 is qualified and Tasks advertisement is enabled.
- Inspector 2.5.0 declares and passes Tasks.
- Codex 0.153.0 does not declare Tasks and uses synchronous fallback.
- Runtime is 59/59 and Rust security is 20/20.
- M3-06/milestone closure remains pending.