## Result

M3 is **not closed**. The final core gate passed **14/14**, and the full gate passed **18 stages before failing at `audit-data`**.

The blocker is environmental and reproducible:

```text
sandbox-exec: sandbox_apply: Operation not permitted
```

The managed Codex sandbox does not permit the nested macOS sandbox required by `scripts/test-audit-data.py`. I did not skip or weaken that stage. Because full uses fail-fast, six later native stages were not executed.

The repo-visible handoff is [M3-07.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-07.md). It records the milestone as blocked rather than claiming closure.

## Consolidated gates

All three required host paths were verified before starting:

- Docker socket: present and a Unix socket.
- `RUST_MCP_E5_DIR`: present.
- `ORT_LIB_LOCATION`: present.

| Command | Exit | Result | Duration | Receipt |
| --- | ---: | --- | ---: | --- |
| `python3 -B scripts/check-architecture.py` | 0 | PASS | 0.02 s final run | Console |
| `python3 -B scripts/gate.py core --report …` | 0 | 14/14 stages | 136.955 s | [M3-core-gate.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-core-gate.json) |
| `python3 -B scripts/gate.py full --report …` | 1 | 18 passed, `audit-data` failed | 2,347.877 s | [M3-full-gate.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-full-gate.json) |

Before the full-gate blocker, the final source bytes passed:

- Workspace tests: **1,066 passed, 0 failed, 115 ignored**, plus one doctest.
- Python gate harness tests: 68 passed.
- Docker security: **4/4**.
- Rust security: **20/20**.
- M2 runtime.
- M3 runtime: **59/59**:
  - nextest 19
  - Tasks lifecycle 4
  - coverage 8
  - SemVer 18
  - mutation 10
- `cargo-audit 0.22.1`: passed, no vulnerabilities; retained the allowed `paste 1.0.15` unmaintained warning.
- `cargo-deny 0.19.7`: advisories, bans and sources passed.

The final standalone fmt, check, Clippy, workspace-test and architecture repetitions also exited 0. The earlier `catalog_sync` failures were confirmed to be a Codex sandbox artifact; they passed in these gates.

Failed receipts were preserved:

- [Core attempts 1–3](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-core-gate-attempt1.json)
- [Full attempt 1](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-full-gate-attempt1.json)
- [Full attempt 2](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-full-gate-attempt2.json)

## Source inventory

The gate recorded:

- **800 inputs**
- **45,936,413 bytes**
- Canonical `source_inputs` SHA-256: `ee32c466518848bfc957ec3225294e922ebdbe8c86af0930080a4b9a432ba307`
- Core start/end inventory: identical.
- Full started with the same inventory; fail-fast prevented its final comparison.

Guest image:

`sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`

Config digest:

`sha256:7d4e58b9e29b2045c13d71542f7892ee071a6886a1b939c4cbfc3ff7ce40dc45`

Plugin versions, binary/source hashes, licences, host-tool versions and Resource schemes are recorded in [M3-07.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-07.md).

## Final matrix

| Area | Status |
| --- | --- |
| M3-01 nextest | Done, qualified |
| M3-02 Tasks | Done, qualified |
| M3-03 coverage | Done, qualified |
| M3-04 SemVer | Done, qualified |
| M3-05 mutation | Done, qualified |
| M3-06 integration | **Blocked** |
| G1 architecture/contract | Blocked by missing product-level task-materialization tests for coverage, SemVer and mutation |
| G2 authority/threat model | Blocked pending formal ADR-064/065 acceptance |
| G3 lifecycle/quotas | Done |
| G4 fixtures/runtime | Done |
| G5 consolidated gate | **Blocked at `audit-data`** |
| G6 upgrade/rollback | Pending dedicated M3 receipt |
| G7 non-release operation | Done |
| G8 independent review | Blocked; final reviews remain `Revise` |
| G9 milestone closure | Blocked |

Full details: [M3-matrix.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-matrix.md).

## Changes

Two source defects discovered by the gate were fixed:

- [codex-model-qualifier.py](/Users/cburgosro/Projects/rust-mcp/scripts/codex-model-qualifier.py): Darwin `libproc` fallback when `/bin/ps` is sandbox-denied. Its focused suite passes 39/39.
- [fix_mutation.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/inspection_runtime/fix_mutation.rs): corrected the stale final inventory oracle from 18 to 22 tools. Its focused Docker selection passes 3/3.

Documentation, receipts and review artifacts were updated. Every authored W5 file and its full SHA-256 is listed in [W5-files.sha256](/Users/cburgosro/Projects/rust-mcp/docs/validation/W5-files.sha256), whose own SHA-256 is:

`6abb964624bbdcac8c55ebf2a17139a27a372628ae026452cf8f28e2a869104d`

Key receipt hashes:

- Core: `9a6c0a896fdd86e79986a3ac8bf35b8f3088798224c9dde4b808abb6fbda0d35`
- Full failed receipt: `e7519c161d9c836428d767fcaa6a8a9839e66a4cfcc2f6487c3e7430c5ae7b6d`
- Handoff: `f4a8fae2fb54845a64abdf60d2bd3786386af8ad9280d92f7d845dc20e89196a`

All 23 snapshots have no W5 byte diff. `gate.py` already invoked exactly one `m3-runtime` stage immediately after M2, so no additional stage wiring was necessary.

## Docker hygiene

Final checks returned:

```text
owned_containers=0
owned_volumes=0
```

The mandatory Docker-security stage reused cached layers but regenerated the local `rust-mcp-probe:m0` manifest/attestation, leaving that probe tag at:

`sha256:8a57560db7ea6336504240ba3a8301a2c69e733b301b73bcb2be07e86bf1ce91`

The approved M3 guest image remained unchanged.

## Remaining closure work

1. Run the full gate from a macOS host context that permits its native `sandbox-exec` child.
2. Add product-level `CreateTaskResult` tests for coverage, SemVer and mutation.
3. Re-run the final independent contract/security reviews.
4. Formally dispose ADR-064 and ADR-065.
5. Produce the dedicated M3 upgrade/rollback receipt for G6.
6. Only then move G1/G2/G5/G6/G8/G9 to satisfied and create any commits or release artifacts.

No commit, merge, push, tag, release, install, download, image pull or image deletion was performed. HEAD remains `52396184e5b53983056791f62d9eecbab3954d15` on `ai/m3-quality`.