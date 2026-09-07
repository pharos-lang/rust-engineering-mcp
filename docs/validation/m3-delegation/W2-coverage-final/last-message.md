## Task

Completed W2: amended ADR-065, qualified coverage against the pinned guest runtime, completed the 55-selection M3 gate, recalibrated Rust security, and recorded current receipts.

## Result

| Cut | Result |
|---|---:|
| Nextest | 19/19 |
| Coverage | 8/8 |
| SemVer | 18/18 |
| Mutation | 10/10 |
| Complete M3 runtime | **55/55** |
| Rust security | **20/20** |

Coverage is qualified. ADR-065 remains `Proposed`, as instructed.

The implementation uses the dedicated target volume read-write only for `CoverageRun` and the three `CoverageReport` phases. The keeper remains read-only; exporters and non-coverage phases never receive it. No seccomp, network, identity, capability, or other mount profile changed.

During qualification, two additional real defects were found and fixed:

- The HTML USTAR validator rejected all genuine archives because `./` was simultaneously required and forbidden. It now accepts only the safe root, safe directory entries, and regular descendants.
- Shared source files were double-counted because LLVM emitted lexical aliases containing `../../`. Paths are now normalized lexically within `/source`; escapes are rejected.

## Calibration

| Oracle | Derived hypothesis | Observed |
|---|---|---|
| Functions | 2/2 | **2/2** |
| Regions | ≥3, at least one uncovered | **8/9** |
| Lines | 100%, informally three lines | **4/4** |
| Shared workspace | One shared aggregate file | **3 canonical files; one `shared.rs`** |
| Zero denominator | No percentage | Plugin reports `no coverage data found`; no artifacts or fabricated percentage |
| Formats | One profdata | JSON, LCOV and HTML produced sequentially from one capture |

The line-count discrepancy is explained: LLVM counts source lines 1–3 plus the executed test body on line 6. The percentage hypothesis was correct; the informal denominator was not.

Observed known-count artifact sizes:

- JSON: 1,949 bytes
- LCOV: 365 bytes
- HTML USTAR: 20,480 bytes

Largest observed HTML fixture: 30,720 bytes.

Identity:

- cargo-llvm-cov 0.9.0
- LLVM 22.1.8-rust-1.98.1-stable
- cargo-llvm-cov SHA-256: `1829d7aff143b565ec996255d6a431770de8713a664b8da50add1c76c1a57a9e`
- llvm-cov SHA-256: `c3865abd81eed0db83ae257b347c4fef88f457bc303cd5ce104d24f49e081272`
- llvm-profdata SHA-256: `c7160d195180796edf7cc4c3f087f72b5425fc9ec2d9734b956a265e506f982e`

## Tests executed

- `cargo test -p rust-engineering-execution ...coverage_mounts_enforce...`
  - Exit 0; 1/1.
- Coverage parser/HTML focused unit tests:
  - Exit 0; all passed.
- `cargo test -p rust-engineering-execution --test coverage_runtime --locked --offline -- --ignored --nocapture --test-threads=1`
  - Exit 0; **8/8**; 133.06 s.
- `RUST_MCP_M3_RUNTIME_OUTPUT=target/m3-runtime-w2-attempt8 python3 -B scripts/test-m3-runtime.py`
  - Exit 0; **55/55**; 1,073.858 s wall time.
  - Receipt: [M3-runtime.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-runtime.json)
  - Receipt SHA-256: `8c271a7cd406cdc78f04b13ddcf36aee924681c79b00ab835043f34bc6a5a3e3`
- `python3 -B scripts/test-rust-execution.py`
  - Exit 0; **20/20**; 535.014 s.
  - Receipt: [M3-rust-security.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-rust-security.json)
- `cargo fmt --check`
  - Exit 0.
- `cargo check --workspace --all-targets --locked --offline`
  - Exit 0; 17.42 s.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`
  - Exit 0; 7.49 s.
- `cargo test --workspace --locked --offline`
  - Exit 0; **1,062 passed, 109 ignored, 0 failed**, 82 summaries.
  - All six `catalog_sync` tests passed. The earlier `EPERM` remains classified as a Codex sandbox artifact.
- `python3 -B scripts/check-architecture.py`
  - Exit 0.

Initial focused diagnostics exposed the HTML validator and zero-denominator issues above. These were not complete runtime-gate attempts; the authoritative fresh attempt passed without failure. Attempts 1–7 remain untouched.

## Evidence

| Oracle | Test |
|---|---|
| Exact coverage access matrix | `coverage_mounts_enforce_the_adr065_access_matrix_and_exact_options` |
| Target absent outside coverage | `coverage_target_is_absent_from_every_non_coverage_phase` |
| Exact 4/4, 8/9, 2/2 counts | `known_counts_fixture_has_exact_line_region_and_function_oracle` |
| Canonical shared-file dedupe | `shared_file_workspace_deduplicates_aggregate_only` |
| Zero denominator never fabricated | `zero_denominator_is_absent_from_percent_metrics` |
| Three formats from one capture | `three_report_formats_derive_from_one_capture` |
| Timeout joined cleanup | `timeout_mid_build_is_blocked_after_joined_cleanup` |
| Cancel/EOF and capacity reuse | `cancel_or_eof_joins_active_child_before_capacity_reuse` |
| Source/report execution and AF_INET denial | `hostile_html_is_retained_only_as_opaque_archive_bundle` |
| Safe USTAR root and descendants | `html_archive_accepts_only_the_tar_root_and_safe_regular_descendants` |

All 23 snapshot files were byte-identical before/after W2; `git diff` over the snapshot directory was empty.

## Docker hygiene

After the focused coverage gate, complete M3 gate, and Rust security gate, both commands returned empty output:

```text
docker ps -aq --filter label=org.rust-mcp.execution=true
docker volume ls -q --filter label=org.rust-mcp.execution=true
```

Final audit:

```json
{"owned_containers":[],"owned_volumes":[]}
```

## Files changed

| File | SHA-256 |
|---|---|
| [rust_gateway.rs](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/src/rust_gateway.rs) | `5da3c6da6026438915a629518948b1cae4a4554726fe16f6fe6a84cf44dfd734` |
| [rust_applied.rs](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/src/rust_applied.rs) | `eef86f179608b95396f62fb4a7b3bef284a80fe3d6ac3aa29de992111d12bf83` |
| [coverage_gateway.rs](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/src/coverage_gateway.rs) | `876f54fc993578d67f4ea74475374eb68273806213b31dcf931324261b01a628` |
| [coverage_json.rs](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/src/coverage_json.rs) | `9a865d6918f807cfb7454411903e898dbd98b70561500b233fbd58b7fff86ef9` |
| [coverage_runtime.rs](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/tests/coverage_runtime.rs) | `b0d61e4f53a60f83188d05ffc044cee21077b5adbe4a30ec7d495d841752555d` |
| [coverage fixture README](/Users/cburgosro/Projects/rust-mcp/fixtures/coverage/README.md) | `b94f32aa106228215b96c74bc2e8c1d4ebc96d524e33d5a3cc7588bae0e2d1a3` |
| [zero-denominator fixture](/Users/cburgosro/Projects/rust-mcp/fixtures/coverage/zero-denominator/src/lib.rs) | `27bd787e1dbab1b7ee3c782473f6018dbd526cc6adbc22212866da5268ef5e40` |
| [ADR-062](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-062-coverage-accounting-and-semver-baselines.md) | `e53562646b85958e7524b5496a25ca5492423a78bc3cc90a45ad8fc5266dc9b8` |
| [ADR-065](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-065-coverage-target-volume.md) | `0f19792527b81aef637dadf1433a2fdbba7e0de52c4c6c744bd9bf4aafb8e109` |
| [tools.md](/Users/cburgosro/Projects/rust-mcp/docs/tools.md) | `3e559968e7e06f29e21468f26dc0801b2536ed9327b5394781310477d4ed01b8` |
| [implementation-status.md](/Users/cburgosro/Projects/rust-mcp/docs/implementation-status.md) | `ac6884bca91158be76907485e7f57c7040671506f0c29ca4095d63c8cdd65716` |
| [M3-03.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-03.md) | `ca034db5d760f2250b784788fd5ca0d3b199ad8a50b1ff19255adadd59943cbe` |
| [M3-matrix.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-matrix.md) | `f561bb45beb765335ac5006fa6a0f8221ea84cc542d3f483afdfab0921508a42` |
| [M3-runtime.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-runtime.json) | `8c271a7cd406cdc78f04b13ddcf36aee924681c79b00ab835043f34bc6a5a3e3` |
| [pre-amendment security receipt](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-rust-security-pre-adr065-amendment.json) | `cb5b7ad6c3fa729ac971b269e3c696c4adc45f7f50bfde52c2e19c9bcd836808` |
| [current security receipt](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-rust-security.json) | `4c55c70a7258f5b9218b77b47e96c5e3fa44d2f89d0221cacf5e84f524f019f3` |

## Risks and open issues

- The 8 MiB artifact bound was exercised only with small fixtures; it remains a conservative security ceiling, not proof of capacity for large workspaces.
- The calibrated workspace export uses the parser’s `workspace` package fallback; aggregate accounting is exact, but richer per-package attribution remains an M3 handoff consideration.
- ADR-065 still needs formal milestone acceptance.
- M3-02 Tasks advertisement/client qualification remains outside W2 and stays disabled.

No W2 blocker remains. No commits or prohibited Docker/image operations were performed.