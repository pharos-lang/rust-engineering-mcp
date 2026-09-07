# S00 — M3 baseline inspection and branch creation

## Task

Package `S00 (pre-M3-01)`: establish the live M3 baseline, leave the checkout on
`ai/m3-quality` at the current `main` HEAD, inventory the workspace and host, verify
M2 closure evidence without changing receipts, map the M3 extension interfaces, and
identify M3-01 Definition-of-Ready blockers. Scope was inspection plus this report;
no product code, receipt, dependency, image, or tool installation was authorized.

## Result

**Inspection complete; requested branch end-state complete; M3-01 is not Ready.**

- Final live branch: `ai/m3-quality`.
- Final live HEAD: `52396184e5b53983056791f62d9eecbab3954d15`.
- `main` is at the same HEAD; M2 merge `7554bccbff2209ae5b3df63b2b1011646586380f`
  exists and is an ancestor; `ai/m2-write-qualification` is exactly
  `ed1d6b21327ca38db2c1b35a9a61e0d74de686c8`.
- This delegate's mandated `git switch -c ai/m3-quality` attempt initially failed
  because its sandbox could not write `.git`. The verbatim error was:

  ```text
  fatal: cannot lock ref 'refs/heads/ai/m3-quality': Unable to create '/Users/cburgosro/Projects/rust-mcp/.git/refs/heads/ai/m3-quality.lock': Operation not permitted
  ```

  During the inspection, the orchestrator-side `S00b-branch` helper created and
  checked out the branch successfully. Its evidence and the live reflog show
  `checkout: moving from main to ai/m3-quality` at `2026-09-05 18:51:43 -0500`.
  I did not create, invoke, or delegate to that helper.
- `cargo check --workspace --all-targets --locked --offline` passes at the live
  checkout: Cargo exit `0`, measured warm incremental duration `0.176 s` (Cargo's
  own reported build duration `0.12 s`).
- The M2 closure verifier passes: all **574** recorded source inputs hash identically.
- The approved immutable Rust image is
  `sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909`.
  Local presence could not be confirmed because the only allowed Docker inspect
  was denied access to the Docker socket.
- The approved image contains Rust/Cargo/rustfmt/Clippy only. It does not provision
  cargo-nextest, cargo-llvm-cov, llvm-profdata/llvm-cov, cargo-semver-checks, or
  cargo-mutants.
- M3-01 DoR remains blocked by unresolved D06 and D17, undecided provisioning and
  plugin identities, no job/task persistence model, and an artifact store whose
  current in-memory quotas and metadata do not satisfy D17/M3.

## Files changed

- Added only `docs/validation/m3-delegation/S00-baseline/report.md`.
- No files under `crates/`, `scripts/`, or `fixtures/` were changed.
- No receipt was changed. No commit, merge, push, publication, install, download,
  Docker build, or Docker run was performed.
- Preserved all pre-existing and concurrently produced untracked content. Initial
  `git status --porcelain` was:

  ```text
  ?? "Claude outputs/"
  ?? docs/prompts/implement-m3-fable-orchestrator.md
  ?? docs/validation/m3-delegation/
  ```

  The third entry is a live discrepancy from the orchestrator's initial two-item
  expectation, but it is the pre-existing delegation evidence tree containing this
  package and the orchestrator's R00/R01/S00b runs; it was not removed or rewritten.
  During this inspection, other orchestrator packages also produced three new
  untracked paths. Final status at report completion was:

  ```text
  ?? "Claude outputs/"
  ?? crates/mcp-server/tests/rmcp_tasks_spike.rs
  ?? docs/adr/ADR-061-private-quality-artifact-store.md
  ?? docs/adr/ADR-062-coverage-accounting-and-semver-baselines.md
  ?? docs/prompts/implement-m3-fable-orchestrator.md
  ?? docs/validation/m3-delegation/
  ```

  Those three concurrent outputs were not authored, executed, or edited by S00.

## Tests executed

- Health check only:

  ```text
  cargo check --workspace --all-targets --locked --offline
  exit: 0
  measured duration: 0.176 s
  cargo output: Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.12s
  ```

  The identical allowed command was repeated to obtain a reliable shell exit code:
  an earlier timing wrapper used zsh's read-only variable name `status`, so Cargo
  completed successfully but the wrapper itself exited `1` with
  `zsh:3: read-only variable: status`. No other Cargo build/test/gate command ran.
- Read-only M2 evidence verification:

  ```text
  python3 -B docs/validation/m2-closure/verify-evidence.py --help
  status passed
  head 52396184e5b53983056791f62d9eecbab3954d15
  source_inputs 574
  runtime_cases 17
  runtime_selections 10
  verified_logs 73
  m1_identical_snapshots 13
  checked_documents 38
  local_link_targets 442
  fences_and_diff_check passed
  script_sha256 fa8fc5f7957737c84298c0fa2c367cfa895ff4b98f33bbeb38e7c3ba9f988802
  ```

  The verifier has no argparse/help branch; `--help` is ignored and therefore ran
  its normal read-only verification. It records the limitation that it checks local
  links/fences and does not execute Cargo or Docker. No receipt was modified.
- Read-only Docker command attempted, exactly within the allowed scope:

  ```text
  /Applications/Docker.app/Contents/Resources/bin/docker \
    --host unix:///Users/cburgosro/.docker/run/docker.sock \
    image inspect sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909 \
    --format '{{.Id}}'
  exit: 1
  permission denied while trying to connect to the docker API at unix:///Users/cburgosro/.docker/run/docker.sock
  ```

## Evidence

### 1. Live Git baseline

Initial and final tracked state agree on the three required commits. The extra
untracked `docs/validation/m3-delegation/` entry is the only repository-fact
discrepancy. Remote configuration is:

```text
origin https://github.com/pharos-lang/rust-engineering-mcp.git (fetch)
origin https://github.com/pharos-lang/rust-engineering-mcp.git (push)
```

All local branch tips at final inspection:

```text
ai/ci-portability-fix 8394eabfa712a5e8896f38f502dfc9866586921c
ai/m0-01-foundation 623653d25ab94822f85d5e2931bb0618492ad0c0
ai/m0-02-domain-contracts 53e07c6fd70347890e514d65d7374a9df7a68b54
ai/m0-03-mcp-stdio 17ef0ab910f0835b1ce53556ebbb23234d715cad
ai/m0-04-project-open 302f972a61305d39626f452b397c36f5b5724ea7
ai/m0-05-execution-gateway ed9234ae4c0e3d30dccea68fd6f203d0dbd3e771
ai/m0-06-sandbox-capabilities 0fee675fd6dd333ceb1f2d1a1231998d404973da
ai/m0-07-mcp-contracts ca0e9fbd2e1845f25b6de8dbcebc31626211b068
ai/m0-08-sqlite-catalog df8d2585d28a1e9163575ac0d68d556a28c3986e
ai/m0-09-local-semantics ef4ee9c590f96a219b744ef3767a34d8c6d3473b
ai/m0-10-fixtures 7852cc1e85d900b910b81cd832dd8eccd8e68d5f
ai/m0-10a-artifacts 2da406fd15414f842a6fe204c1cb855d22f4c612
ai/m0-11-local-ci df01d3f8ea4504142603eb8e8361333809221b93
ai/m0-12-foundation-gate ce574a74bc9d64f791cdae6f4a3d56212831dbce
ai/m0-m1-closure-prompt a4cf6682c655316b31936651165fd90102134f00
ai/m1-01-gateway-io 329e85dd90e951c48ae3b8309df534e775d8f863
ai/m1-01-joined-workers b01adc6a9c8d9ab456b98d5499800e40982b18f1
ai/m1-01-project-inspect 52139e6cb44eb3ee76db98b59a2e7b4f627a992a
ai/m1-01-runtime d247d37bff36f26375f83e34046afe5041c1f1dd
ai/m1-01-rust-gateway 34b425adcd952becf59c1bbf63043dfaf6eb95bc
ai/m1-01-source-capture b951ea4bbaca99a66c61cd7a836e516fd56eef86
ai/m1-01-workers dc53c72b664fbb5ada93c6dac520e5295e9b908f
ai/m1-02-toolchain f6c5c592b20555329ed4d03b21853804ab7a3c0b
ai/m1-03-check 96fc984fe93b0839fd885fb597b30ebc391a56ce
ai/m1-04-fmt 5e97806b34ca9b2457a2e42ea77995ef2d6e1891
ai/m1-05-clippy 464fffb36a88646fa9f67abab6b409838c2587a0
ai/m1-06-test f10304db1b7055a5acdc5a6e5826a406c53ba757
ai/m1-07-audit be74318a5bf01122bd3fb5d77289932b3764f280
ai/m1-08-explain 571469d02ab6849a1f8c4c221ea39217f41c36b0
ai/m1-09-integration-evidence c9e511b81f285b773888e328d759e591bdd83613
ai/m1-09-quality-gate 983e5adbbdd490443d3daafeeed8a43c2394df16
ai/m1-10-catalog-import 9f28179ec0880f596fcbe17c5f65d8e988cbc234
ai/m1-10-integration-evidence ddba9c00386c40908a8d85e81e4899623e536979
ai/m1-11-catalog-status 261b0c4b7cbcfb0e7e1ce9a4c3c4435249d91d4a
ai/m1-11-integration-evidence 78cae1cc37173e823b3608ee2a09234e786e74c8
ai/m1-12-crate-search d7bc53875b417807e076f92d82ebaa7886137416
ai/m1-12-integration-evidence b712469fb344e2962097b80ce1afbfc225080d39
ai/m1-13-crate-inspect 08e41f3a8c120e6920058b2606cf8781e1b7f11e
ai/m1-13-integration-evidence 0954569778e00f17b00b5969e040c36d8ee4c535
ai/m1-14-cli-doctor a72216d44433b742d4c67165ae167948a471a43d
ai/m1-14-integration-evidence ae1441993c3adc9266639e86da05e17bb18e77f3
ai/m1-15-candidate-evidence 7a0e9de37e77cf2b5d20f7f91612c9425351c90b
ai/m1-15-offline-candidates 20e7e70fd6ebc7d4691505e8edae67421987309f
ai/m1-15-release-preparation 1cb5d67a66430588801515981b495ad507cd6025
ai/m1-16-postmerge-evidence 712a66509c38c99004c8ccbce946aebad0417d0f
ai/m1-16-utility-experiment e7af63f7d0c144c99270c0c18c2d9535f4caf3b5
ai/m1-17-postmerge-evidence b3782561ca8941898911aed714f55c8f257455dc
ai/m1-17-release-qualification 84d927d27c3106ad7530a24a23d58e80d627e818
ai/m1-closure-evidence cd71e23b93ccb4d64975e662f1d3fb77d2e9beb9
ai/m2-m8-roadmap 8b35cf6b3eec11dd4cda4d65ea8825ac7c045ba3
ai/m2-write-qualification ed1d6b21327ca38db2c1b35a9a61e0d74de686c8
ai/m3-quality 52396184e5b53983056791f62d9eecbab3954d15
ai/macos26-ci-runner 83d8e20271f88ba2ab096815c9f3aaee7d07f88a
ai/native-ci-environment-fix 185feb200cb310e19fd91b5aa85d6b605037a67e
ai/publication-foundation 92a3bbded2c783df0801b05c9abedbb64ce94cc1
ai/publication-receipt 5709d5c03a9db418b3faa950407b1b6d17bcce1e
ai/publication-sanitizer-fix e9532dab7aa49290bae2a6092ba902c989b0993f
ai/sonarcloud-actions 3638904902750eb506bd292147699f4655b5144b
ai/supply-chain-ci-fix e6024af1664dd5e93107274b0a681670219a85c6
ai/windows-catalog-fixture 167cf2b382d2557fa32d086a9c6c421e7a0197f6
ai/windows-cli-path-fixture 42c27c61851967ad6d1fb65a0681d31d421f0c13
ai/windows-utf8-architecture-check b6fd06281512e511146cc6d15444c0d630f70448
main 52396184e5b53983056791f62d9eecbab3954d15
```

### 2. Workspace inventory

`Cargo.toml:2-11` defines eight crates: `domain`, `application`, `mcp-server`,
`project-adapter`, `execution-adapter`, `catalog-adapter`, `semantic-adapter`, and
`artifact-adapter`. Workspace package version is `0.2.0-dev`, edition `2024`, and
MSRV `1.98.1`. `rust-toolchain.toml:1-4` pins channel `1.98.1`, minimal profile,
with `clippy` and `rustfmt`.

Key locked versions (direct version first; multiple entries mean Cargo.lock also
contains a transitive major/minor):

| Dependency | Locked version(s) |
| --- | --- |
| rmcp | 3.2.0 |
| tokio | 1.53.1 |
| serde | 1.0.229 |
| schemars | 1.2.2 direct; 0.9.0 transitive |
| jsonschema | 0.53.0 |
| rustix | 1.1.4 |
| toml_edit | 0.25.13+spec-1.1.0 |
| sha2 | 0.11.0 direct; 0.10.9 transitive |

Test-bearing file inventory criterion: every Rust file below either resides under a
crate `tests/` tree or contains a test attribute/module marker in `src/`. Counts are
therefore test-bearing source files, not the number of test cases or Cargo test
targets.

- `application` — 18:
  `tests/artifact_access.rs`, `tests/audit.rs`, `tests/catalog_context.rs`,
  `tests/check.rs`, `tests/clippy.rs`, `tests/crate_inspect.rs`,
  `tests/crate_search.rs`, `tests/execution.rs`, `tests/explain.rs`,
  `tests/format.rs`, `tests/inspection.rs`, `tests/mutation.rs`,
  `tests/quality.rs`, `tests/registry.rs`, `tests/resolution.rs`,
  `tests/source.rs`, `tests/test_run.rs`, `tests/toolchain.rs`.
- `artifact-adapter` — 2: `src/lib.rs`, `src/tests.rs`.
- `catalog-adapter` — 13: `src/audit.rs`, `src/audit/lock.rs`,
  `src/audit/tests.rs`, `src/bundle.rs`, `src/bundle/floor.rs`,
  `src/bundle/tests.rs`, `src/inspect.rs`, `src/lib.rs`, `src/tests.rs`,
  `tests/catalog.rs`, `tests/crate_inspect.rs`, `tests/crate_search.rs`,
  `tests/hybrid.rs`.
- `domain` — 14: `src/artifact.rs`, `src/check.rs`, `src/clippy.rs`,
  `src/source.rs`, `src/test_run.rs`, `tests/catalog_context.rs`,
  `tests/contracts.rs`, `tests/crate_inspect.rs`, `tests/crate_search.rs`,
  `tests/diagnostics.rs`, `tests/freshness.rs`, `tests/manifest_edit.rs`,
  `tests/mutation.rs`, `tests/quality.rs`.
- `execution-adapter` — 20: `src/applied.rs`, `src/applied_tests.rs`,
  `src/capabilities.rs`, `src/cargo_diagnostics.rs`, `src/format_output.rs`,
  `src/lib.rs`, `src/mutation_archive.rs`, `src/mutation_gateway.rs`,
  `src/project_inspection.rs`, `src/project_metadata.rs`,
  `src/resolution_gateway.rs`, `src/rust_applied.rs`, `src/rust_calibration.rs`,
  `src/rust_gateway.rs`, `src/rust_gateway/test_runtime.rs`,
  `src/source_archive.rs`, `src/state.rs`, `src/supervisor.rs`,
  `src/toolchain_metadata.rs`, `tests/gateway.rs`.
- `mcp-server` — 67: `src/capabilities.rs`, `src/cargo_vendor_cli.rs`,
  `src/catalog_sync.rs`, `src/catalog_sync/tests.rs`, `src/doctor.rs`,
  `src/doctor/tests.rs`, `src/host_config.rs`, `src/mutation_cli.rs`,
  `src/stdio/admission.rs`, `src/stdio/auditing.rs`,
  `src/stdio/auditing/provider.rs`, `src/stdio/auditing/tests.rs`,
  `src/stdio/budget.rs`, `src/stdio/catalog.rs`,
  `src/stdio/catalog/provider.rs`, `src/stdio/catalog/provider/tests.rs`,
  `src/stdio/catalog/tests.rs`, `src/stdio/check.rs`,
  `src/stdio/check/tests.rs`, `src/stdio/clippy.rs`,
  `src/stdio/clippy/tests.rs`, `src/stdio/contract.rs`,
  `src/stdio/crate_inspect.rs`, `src/stdio/crate_inspect/tests.rs`,
  `src/stdio/crate_search.rs`, `src/stdio/crate_search/tests.rs`,
  `src/stdio/explaining.rs`, `src/stdio/explaining/tests.rs`,
  `src/stdio/format.rs`, `src/stdio/format/tests.rs`,
  `src/stdio/inspection.rs`, `src/stdio/inspection/tests.rs`,
  `src/stdio/mutation.rs`, `src/stdio/mutation/audit.rs`,
  `src/stdio/mutation/semantic_input.rs`, `src/stdio/project.rs`,
  `src/stdio/quality.rs`, `src/stdio/quality/tests.rs`,
  `src/stdio/resources.rs`, `src/stdio/resources/tests.rs`,
  `src/stdio/testing.rs`, `src/stdio/testing/tests.rs`,
  `src/stdio/toolchain.rs`, `src/stdio/toolchain/tests.rs`,
  `src/stdio/workers.rs`, `tests/capabilities_cli.rs`,
  `tests/catalog_cli.rs`, `tests/catalog_cli/bundle_fixture.rs`,
  `tests/catalog_status.rs`, `tests/cli.rs`, `tests/crate_inspect.rs`,
  `tests/crate_inspect/fixture.rs`, `tests/crate_search.rs`,
  `tests/crate_search/fixture.rs`, `tests/doctor.rs`,
  `tests/inspection_runtime.rs`, `tests/inspection_runtime/audit.rs`,
  `tests/inspection_runtime/dependency_mutation.rs`,
  `tests/inspection_runtime/explain.rs`,
  `tests/inspection_runtime/fix_hostile.rs`,
  `tests/inspection_runtime/fix_mutation.rs`,
  `tests/inspection_runtime/format_mutation.rs`,
  `tests/inspection_runtime/mutation.rs`,
  `tests/inspection_runtime/mutation_concurrency.rs`,
  `tests/inspection_runtime/quality.rs`,
  `tests/inspection_runtime/terminal_plan.rs`, `tests/protocol.rs`.
- `project-adapter` — 14: `src/catalog_store.rs`,
  `src/filesystem/macos/mutation.rs`, `src/filesystem/macos/source.rs`,
  `src/manifest.rs`, `src/semantic_delta.rs`, `tests/cargo_oracle.rs`,
  `tests/cargo_vendor.rs`, `tests/catalog_store.rs`, `tests/filesystem.rs`,
  `tests/host_snapshot.rs`, `tests/manifest_edit.rs`,
  `tests/mutation_store.rs`, `tests/source.rs`,
  `tests/support/native_mutation.rs`.
- `semantic-adapter` — 4: `src/index.rs`, `src/index/persistence.rs`,
  `src/model.rs`, `tests/local.rs`.

### 3. M2 closure contrast

`docs/validation/m2-closure/verify-evidence.py` recalculates the tracked-input
inventory and hashes, checks the recorded HEAD and full-gate/script/binary/client
identities, validates 73 logs, verifies the 13 M1 snapshots remained identical,
checks 38 documents and 442 local links, and rejects patch/diff fences in closure
documents. Against the current checkout it returned `status passed`, HEAD
`52396184...`, `source_inputs 574`, and full-gate SHA-256
`05f0391041e053309e8924183600dcfbd8cd41b925afadb2edca7fcb73e43ea5`.
Therefore all 574 M2 inputs still hash identically to the closure receipt.

### 4. Provisioning and environment inventory

#### Approved guest construction

`fixtures/rust-runtime/sources.json:2-43` pins Linux ARM64, Rust 1.98.1, a Debian
ARM64 base digest, and five official static.rust-lang.org component archives:
rustc, rust-std, Cargo, rustfmt-preview, and clippy-preview. Each has an explicit
SHA-256. `provision.py:19-50` refuses unexpected/symlinked context entries,
downloads only missing pinned archives, verifies SHA-256, copies the Dockerfile,
pulls the pinned base, records its inspection, and builds the output image. This is
an explicit trusted-host operation and is not imported by the MCP runtime.

`Dockerfile:1-10` verifies all archives, installs them under `/opt/rust`, and makes
a final `scratch` image containing only the prepared filesystem with user
`65534:65534` and workdir `/work`. `verify.py:17-58` checks immutable image
configuration/platform, runs version/component/digest/dpkg probes under bounded,
read-only, network-none containers, and requires exactly the five component names.
`README.md` and `docs/validation/M2-image-config.json:4` record immutable image ID
`sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909`.

The Docker socket path exists, but Docker API permission was denied, so image-local
presence is **unconfirmed**, not absent.

#### M2 full-gate paths and current existence

The full-gate code admits only explicit `RUST_MCP_TEST_SOCKET`, `RUST_MCP_E5_DIR`,
and `ORT_LIB_LOCATION`; the exact ORT path is serialized in the prior full-gate
receipt while the final M2 JSON does not itself serialize those environment values.
The paths used/inherited for the M2 full qualification are:

| Variable | Recorded path | Exists now |
| --- | --- | --- |
| `RUST_MCP_TEST_SOCKET` | `/Users/cburgosro/.docker/run/docker.sock` | yes (socket path) |
| `RUST_MCP_E5_DIR` | `/private/tmp/rust-mcp-e5-m009/onnx` | yes |
| `ORT_LIB_LOCATION` | `/Users/cburgosro/Library/Caches/ort.pyke.io/dfbin/aarch64-apple-darwin/612739f75438dc0a075461e1fb454226b4a1eb175e60a7271ba966bbbb972cd4` | yes |

The missing serialization of exact env values in `M2-full-gate.json` is a provenance
gap; existence now does not prove byte identity or usability.

#### Host commands

| Command | Path/status | Version output |
| --- | --- | --- |
| cargo-audit | `~/.cargo/bin/cargo-audit` | `cargo-audit 0.22.1` |
| cargo-deny | `~/.cargo/bin/cargo-deny` | `cargo-deny 0.19.7` |
| cargo-nextest | `/opt/homebrew/bin/cargo-nextest` | `cargo-nextest 0.9.143` |
| cargo-llvm-cov | absent from PATH | absent |
| cargo-semver-checks | absent from PATH | absent |
| cargo-mutants | `~/.cargo/bin/cargo-mutants` | `cargo-mutants 27.0.0` |
| llvm-profdata | absent from PATH | absent |
| llvm-cov | absent from PATH | absent |
| Go | PATH | `go version go1.27.1 darwin/arm64` |
| Python3 | PATH | `Python 3.14.7` |
| Node | PATH | `v24.15.0` |
| npx | PATH | `11.12.1` |
| claude | PATH | `2.1.261 (Claude Code)` |
| codex | PATH | `codex-cli 0.153.0` (also warned it could not create PATH aliases under this sandbox) |
| agy | PATH | `1.1.27` |

#### Guest plugin gap

The Dockerfile, sources manifest, and verifier prove that the approved guest has no
provisioned `cargo-nextest`, `cargo-llvm-cov`, `llvm-tools-preview`
(`llvm-profdata`/`llvm-cov`), `cargo-semver-checks`, or `cargo-mutants`. A host binary
does not qualify a guest capability.

#### Provisioning proposal — owner authorization required; no action taken

The following is a request for authorization, not approval or implementation. The
local R01 research capture records current release metadata, but every selected
guest version, upstream checksum, attestation, license file, and extraction layout
must be independently reverified against official upstream before authorization.

| Component | Official upstream | Candidate Linux ARM64 release asset | SPDX/license | Verification status |
| --- | --- | --- | --- | --- |
| cargo-nextest | `nextest-rs/nextest` / `nexte.st` | GNU `cargo-nextest-0.9.143-aarch64-unknown-linux-gnu.tar.gz`; musl `cargo-nextest-0.9.143-aarch64-unknown-linux-musl.tar.gz` | `Apache-2.0 OR MIT` | Version/assets/hashes and license **TO BE VERIFIED before authorization**; local R01 records 0.9.143 and both assets. Prefer musl only after runtime/fixture qualification. |
| cargo-llvm-cov | `taiki-e/cargo-llvm-cov` | `cargo-llvm-cov-aarch64-unknown-linux-gnu.tar.gz` or `cargo-llvm-cov-aarch64-unknown-linux-musl.tar.gz` under release `v0.9.0` | `Apache-2.0 OR MIT` | Version/assets/hashes/attestation **TO BE VERIFIED before authorization**. Local Cargo source and R01 record 0.9.0. |
| cargo-semver-checks | `obi1kenobi/cargo-semver-checks` | `cargo-semver-checks-aarch64-unknown-linux-gnu.tar.gz` or `cargo-semver-checks-aarch64-unknown-linux-musl.tar.gz` under candidate release `v0.50.0` | `Apache-2.0 OR MIT` | Exact version/assets/hashes **TO BE VERIFIED before authorization**. Stable-Rust-1.98.1 compatibility needs a positive guest fixture. |
| cargo-mutants | `sourcefrog/cargo-mutants` / `mutants.rs` | No Linux ARM64 asset is recorded for candidate `v27.1.0`; upstream assets are x86_64-only, so an owner-approved reproducible source build would be required rather than inventing an ARM64 asset name. | `MIT` | Version, absence of ARM64 assets, source dependency closure/hash, and license **TO BE VERIFIED before authorization**. Host 27.0.0 is not a guest qualification. |
| llvm-tools-preview | Rust project distribution at `static.rust-lang.org` | `https://static.rust-lang.org/dist/2026-09-03/llvm-tools-1.98.1-aarch64-unknown-linux-gnu.tar.xz` and `.sha256`; package name `llvm-tools-1.98.1-aarch64-unknown-linux-gnu.tar.xz` | `Apache-2.0 WITH LLVM-exception` for LLVM tools; verify bundled notices | URL/date/name/hash/license **TO BE VERIFIED before authorization** against the Rust 1.98.1 channel manifest. R01 records candidate SHA-256 `caaf950c65f3e428247dbe9c173d142b7072b2134962a61924c01e39f6b6dc1e`, not independently adopted here. |

Proposed authorized change:

1. Add closed, typed source entries for the four plugin distributions plus
   `llvm-tools-preview` to `fixtures/rust-runtime/sources.json`, including upstream
   URL, release/tag, target, SHA-256, license/SPDX, notice source, expected archive
   members, and executable digest. Because cargo-mutants has no ARM64 asset, either
   authorize a separate reproducible source-build stage with a fully pinned vendored
   dependency closure or defer `rust.mutation.test`; do not download/build it at MCP
   runtime.
2. Extend `provision.py` to fetch/verify the new closed entry kinds and construct an
   owner-inspectable private build context. Add a Dockerfile layer that installs
   verified plugin executables and Rust LLVM tools into `/opt/rust/bin` and the
   Rust sysroot layout expected by cargo-llvm-cov. Keep the final image immutable,
   non-root, network-independent, and without package managers or installers.
3. After those reviewed file changes, the explicit owner command would remain:

   ```text
   python3 fixtures/rust-runtime/provision.py \
     --docker /Applications/Docker.app/Contents/Resources/bin/docker \
     --host unix:///Users/cburgosro/.docker/run/docker.sock \
     --output <new-owner-private-output-directory>
   ```

   This command currently expects Rust installer tarballs, so it must not be run
   against plugin assets until the schema/extraction changes are reviewed.
4. Record the new immutable image ID and every binary/version/digest/license in a
   new validation receipt. Amend ADR-031 rather than silently replacing its approved
   identity; decide D06/D17/D18 in real ADRs; update gateway image identity,
   closed-command argv/env/profile fingerprints, calibration, and quarantine rules.
5. Validate fixed-path `--version` output, executable hashes, SBOM/notices, no missing
   dynamic libraries, no network, RO host source, bounded writable tmpfs, stdout/
   stderr and artifact limits, hostile parser inputs, absence behavior, cancel/
   timeout kill-and-join, cleanup/quarantine, and positive native fixtures for every
   advertised tool. Re-run proportional runtime/client gates and preserve the M2
   receipt rather than rewriting it.

### 5. M3 architecture and ownership map

#### a. Execution gateway and closed command set

- `crates/domain/src/rust_execution.rs:1-15`: `RustCommand` is the closed domain
  enum. Existing variants are `Metadata`, `FormatCheck`, `TestProject(TestOptions)`,
  `ClippyProject(ClippyOptions)`, `Check`, `CheckProject(CheckOptions)`,
  `CompilerVersion`, `Explain(DiagnosticCode)`, `CargoVersion`, and
  `InstalledComponents`. M3 adds typed variants here only after D06/D18/tool
  provisioning decisions; arbitrary argv remains prohibited.
- `crates/application/src/execution.rs:7-24,27-34,37-75`: `ExecutionError`,
  `ExecutionPort::execute`, and `admit_execution` are the application boundary and
  common result-admission mapping.
- `crates/execution-adapter/src/rust_gateway.rs:6-15,28-154,156-172`: immutable
  image identity, `Phase`, and the sole `program`/`arguments`/environment mapping.
  Each enum becomes a fixed `/opt/rust/bin/{cargo,rustc}` or `/usr/bin/{tar,cat}`
  entrypoint and typed args; environment is rebuilt from a fixed allowlist with
  offline Cargo and fixed target/home/tmp paths.
- `rust_gateway.rs:222-254,315-471`: implementation/configuration fingerprints,
  `RustGateway`, construction, calibration flags, and quarantine state.
  `project_inspection.rs:23-67,82-111` owns the lazy one-time calibration latch:
  verified/calibrating/calibration-failed state and quarantine are fail-closed.
- `rust_gateway.rs:473-532`: Docker argv fixes `--pull=never`, runc, network none,
  read-only root, no capabilities, no-new-privileges, private IPC/cgroup namespace,
  PID 128, one CPU, 1 GiB memory/swap, 1 MiB shm, no log driver/healthcheck,
  `/work` tmpfs 512 MiB, `/tmp` tmpfs 64 MiB, fixed user and seccomp; source volume
  is writable only for ingest and read-only for execution.
- `rust_gateway.rs:652-718,719-903`: phase creation/inspection, supervisor execution,
  post-inspection, top-level busy lock, identity rechecks, source USTAR transfer,
  common deadline, cleanup-before-result, UTF-8/bounded output conversion, and
  execution fingerprint.
- `crates/execution-adapter/src/supervisor.rs:26-44,57-68,121-281`: `ChildGuard`
  kill+waits on drop; `Readers` always signals and joins both reader threads;
  `run_with_input` captures stdout/stderr concurrently with independent truncation,
  polls cancellation/deadline/output excess, kills and waits before receiving both
  streams, and converts a late drain overflow to `OutputLimit`. The Docker container
  is the process-tree boundary; gateway cleanup then force-removes owned containers.
- `rust_gateway.rs:593-650,755-771`: cleanup attempts every owned container before
  its volume and latches quarantine on uncertainty; an active busy lock or
  calibration/quarantine prevents admission.

#### b. Worker admission and MCP lifecycle

- `crates/mcp-server/src/stdio/workers.rs:12-25,50-81,84-200`: `Workers` has exactly
  one `Semaphore` permit (`new`: line 91), no queue, and fail-fast `try_acquire_owned`
  admission (`admit`: lines 177-189). `Control` combines request, session, local-drop,
  and deadline cancellation. `run_joined` (140-175) keeps the permit on the blocking
  thread and awaits actual completion/cleanup; `run` (97-135) may return its async
  waiter earlier, while the blocking thread still retains the permit.
- `stdio/admission.rs:21-22,82-100,125-177`: MCP framing admission has independent
  16-slot request, notification, and send semaphores, also fail-fast; send deadline
  is 10 s. `stdio/budget.rs:14-17` caps a line at 1 MiB, reads in 8192-byte chunks,
  and gives each frame 10 s.
- `stdio/testing.rs:30,507-522`: `rust.test` has a 120 s outer deadline, rejects
  before bootstrap readiness, and uses `Workers::run_joined` around the entire
  application/gateway operation. ADR-037 additionally closes the test command and
  uses 30 s default / 60 s maximum execution limits.
- `stdio/quality.rs:33,793-809`: `rust.quality.gate` has a 240 s outer deadline,
  readiness guard, and the same joined single-worker path. ADR-040 uses one captured
  source and sequential fixed stages, with 30 s per command inside the 240 s call.
- `stdio/project.rs:16,274-285`: `rust.project.open` uses a 10 s deadline and the
  non-joined `run` path. `stdio/resources.rs:141-190` gives Resource reads 10 s and
  uses `run_joined`. `stdio/mutation.rs:45,808-823` gives M2 operations 240 s and
  uses `run_joined`.
- `stdio.rs:583-619`: shutdown grace is 240 s when Rust runtime is enabled (12 s
  otherwise); the Tokio runtime then gets 100 ms. Bootstrap readiness is set only
  after rmcp serving starts. There is no separate named “bootstrap timeout” constant:
  the roadmap's 10 s bootstrap constraint maps to existing framing/project-open
  limits, while expensive tools reject until ready.

#### c. Artifacts

- `crates/domain/src/artifact.rs:8-83`: validated opaque `ArtifactId`,
  `ArtifactMetadata` (`id`, `owner`, SHA-256, size, truncation, created/expires),
  borrowed `ArtifactView`, and `ArtifactError`.
- `crates/application/src/artifact.rs:7-32`: streaming `ArtifactInput` and
  `ArtifactStore::{capture,read,remove,retain_owners,revoke_owner,cleanup}` ports.
- `crates/application/src/artifact_access.rs:8-151`: 256 KiB content bound,
  owner/ProjectRef authorization, expiry/retention checks, read without touch for
  group validation, and final authorized ProjectRef touch.
- `crates/artifact-adapter/src/lib.rs:14-52,64-70,108-203`: current
  `MemoryArtifactStore<HashMap>` defaults are 1 MiB input, **256 KiB output/artifact,
  16 MiB global, 1 MiB/owner, 256 artifacts global, 64/owner, TTL 3600 s**. Admission
  reserves a full output budget before generation; reads filter by owner.
- `crates/mcp-server/src/stdio/resources.rs:26-108,110-190`: URI format is
  `rust-artifact://<project_ref>/<artifact_id>`. Parsing is strict; bytes are base64
  blobs marked private/no-cache. `Resources` owns the mutex-protected memory store,
  resolves the current ProjectRef owner, and serves only authorized reads. MCP
  advertises Resources but does not globally list artifact URIs.
- `crates/application/src/validation.rs:71-216,264-417`: quality-gate logs are
  captured as artifacts. Publication collects pending IDs, re-reads each without
  touch, revalidates source/owner/metadata, makes one final authorization/touch
  commit point, and removes all pending artifacts on any publication error. Optional
  quota omission is represented honestly rather than evicting already promised data.

#### d. M2 mutation staging to reuse

- `crates/execution-adapter/src/mutation_gateway.rs:13-84`: the private writable copy
  is a Docker local volume backed by tmpfs with exact options
  `size=64m,nr_inodes=8192,uid=65534,gid=65534,mode=0700,nosuid,nodev,noexec`.
  `MutationPhase::{Guardian,Ingest,Format,Fix,Export}` fixes programs and argv.
- `mutation_gateway.rs:150-178,181-251,656-886`: volume inspection must reproduce
  driver/scope/options/owner/labels; containers preserve the Rust gateway limits.
  Ingest/Format/Fix mount `/source` writable; Guardian/Export mount it read-only.
  Fix alone gets a 256 MiB executable `/target` tmpfs and
  `seccomp-rust-fix.json`; the profile adds the narrowly required loopback-related
  socket syscalls while Docker network remains none.
- A non-writable guardian keeps the volume alive while each writer is removed.
  Source bytes enter as bounded USTAR over stdin. All writable containers are
  removed before an Export container creates a deterministic USTAR on stdout.
  The host decodes that archive, compares it with the captured source, accepts only
  the closed mutation scope, and then cleans containers/volume before returning.
- `mutation_gateway.rs:459-520` gives cleanup a separate 10 s never-cancel budget
  and quarantines the gateway on uncertainty. `rust_applied.rs:282-439` verifies the
  applied mutation container identity, mounts, seccomp, user, limits and writable
  phase rules. `applied.rs:78-103` is the earlier probe-profile verifier, not the
  M2 mutation exporter.
- `mutation_archive.rs:9,42-71,174-267` caps archives at 24 MiB, emits/accepts a
  closed USTAR profile, fixed ownership and modes, regular files/directories only,
  and rejects links, extensions, duplicates, path/scope changes and oversized data.
  M3-05 should extend this staging lifecycle rather than introduce host-writable
  execution or an archive-selected host write.

#### e. Source capture and two ProjectRefs

- `crates/application/src/source.rs:8-43`: `ProjectSourceBackend::capture` and
  `ProjectRegistry::source_inner` resolve/revalidate immediately before capture and
  again after capture; changed identity invalidates the stale lease, and cancellation
  does not touch it.
- `crates/application/src/inspection.rs:29-84`: `ProjectInspectionPort::inspect` is
  the reference template: resolve identity, capture once, execute over captured
  bytes, build provenance/fingerprint, then final resolve/touch before publication.
- `crates/application/src/lib.rs:30-48,67-74,105-183`: the registry owns opaque
  `ProjectRef -> Entry<Lease>` mappings and only the project adapter opens/revalidates
  directory capabilities. For SemVer, one joined job can resolve/capture baseline
  and candidate refs separately, retain both captured bundles/identities, and
  revalidate both before publication. This gives two authorized snapshots but is
  not atomic across two roots; the M3 contract must define ordering, compatible
  package/features/target identity, changed-ref behavior, and final dual revalidation.

#### f. MCP contract machinery

- There is one file, `crates/mcp-server/src/stdio/contract.rs`, not a `contract/`
  directory. `ToolOutput` and generic `Contract<I,O>` are at lines 8-20;
  `Contract::new` derives strict schemars schemas and compiles jsonschema validators
  (21-42), `decode` validates then deserializes (44-51), and `encode` validates the
  output and uses rmcp structured constructors (53-69). The SDK constructors emit
  `structuredContent` and an identical JSON `TextContent`; lines 159-183 test that
  mirror and distinguish product failure from MCP operational error.
- `crates/mcp-server/src/stdio.rs:75-211` owns `EngineeringServer`, `get_tool`,
  `list_tools`, and `call_tool`. A new tool needs a typed module/contract, a server
  field initialized at startup, an ordered `get_tool`/`list_tools` entry, and a
  `call_tool` dispatch arm. It must not be advertised before the implementation is
  live.
- `stdio.rs:47-53` explicitly supports five protocol versions:
  `2024-11-05`, `2025-03-26`, `2025-06-18`, `2025-11-25`, and `2026-07-28`.
  `crates/mcp-server/tests/protocol.rs:15-16,315-426,445-468,680-827` drives all five,
  compares schemas to snapshots, checks ordered 18-tool inventory, verifies
  structured/text mirrors, initialization/version negotiation and fallback.
- The 18 guarding snapshots in `crates/mcp-server/tests/snapshots/` are:
  `project-open-tool.json`, `project-inspect-tool.json`,
  `toolchain-inspect-tool.json`, `check-tool.json`, `format-tool.json`,
  `clippy-tool.json`, `test-tool.json`, `audit-tool.json`, `explain-tool.json`,
  `quality-tool.json`, `catalog-status-tool.json`, `crate-search-tool.json`,
  `crate-inspect-tool.json`, `manifest-patch-tool.json`, `fmt-apply-tool.json`,
  `fix-apply-tool.json`, `dependency-add-tool.json`, and
  `dependency-remove-tool.json`.

#### g. Gate and client drivers

- `scripts/gate.py:114-180` creates a source-bound receipt and runs 14 core stages:
  fmt, check, clippy, workspace tests, doctests, architecture, gate-reporting,
  release-artifact tests, release-smoke tests, Codex-qualifier tests, vendor,
  cargo-fixtures, audit, and deny. Full adds 10 stages: Docker security, Rust
  security, M2 runtime, audit-data, semantic, catalog, catalog-status, crate-search,
  crate-inspect, and doctor. `gate.py:81-112,132-143,180-184` snapshots tracked
  inputs, requires the three full-gate env paths, and rejects any source change
  during the gate. M3 stages must append without rewriting M2 closure artifacts or
  changing M1/M2 stage semantics.
- `scripts/test-rust-execution.py:13-182` requires an explicit socket, resolves the
  exact Cargo binary, and runs each hard-coded ignored Docker selection separately:
  `cargo test --locked --offline -p <package> <target> <exact-selection> -- --exact
  --ignored --nocapture --test-threads=1`. It requires exactly one passing test per
  selection and then validates calibration/tool receipts and exact case counts.
- `scripts/test-m2-runtime.py:15-80` uses the same sanitized env and exact ignored
  invocation pattern for 10 selections / 17 cases, hashing its source/config inputs
  and saving a running/final receipt. It does not discover Docker tests dynamically.
- `scripts/test-m2-clients.py:26-60,260-345` drives the vendored stock MCP Inspector
  2.5.0 CLI through Node for ordered 18-tool discovery/schema equality, project open,
  and default-deny checks for all five M2 tools. `:354-521` drives stock Claude Code
  with `claude-sonnet-5`, medium effort, stream-json, restricted, strict MCP config,
  no session persistence, no builtin tools, an exact allowed MCP tool list, and a
  model-directed preview/commit/reopen/receipt flow; it validates model identity,
  call/result pairing, final bytes, and Docker cleanup.
- `scripts/codex-model-qualifier.py:188-223,244-295,684-729,895-918` validates a
  closed plan for `gpt-5.6-sol`, medium, `codex-cli 0.153.0`, generates the exact
  app-server schemas, constructs typed workspace-write/no-network/never-approval
  requests, stages only authenticated executables/config, and drives
  `codex app-server --stdio --strict-config` via `thread/start` then `turn/start`.
  It verifies settings/model/provider/tool allowlists, protocol events, output,
  process identity, and joined cleanup for repair and missing-runtime phases.

#### h. Documentation extension points

- `docs/tools.md` uses a heading per public tool, followed by purpose/authorization,
  strict input/output contract, behavior/evidence, failure/limit semantics, and
  security/runtime notes. M2 groups five tools under “Escritura local M2” with a
  common preview/commit/receipt lifecycle and tool-specific subsections. M3 should
  preserve the same public-vs-internal distinction and document task fallback and
  Resource formats per tool.
- `docs/validation/M2-matrix.md:1-21` begins with milestone status and a three-column
  `Elemento | Estado | Evidencia y límite` table linking every cut/decision to live
  tests and receipts. It then records qualification limits and reproducible
  experiments. M3 needs a separate matrix/receipts, not edits that mutate M2 closure.
- `docs/adr/README.md` is the tracked ADR index. At initial inspection its highest
  assigned number was ADR-059, so **the next free ADR number is ADR-060**. Concurrent
  orchestrator work later created untracked Proposed drafts ADR-061 and ADR-062;
  ADR-060 remains numerically free and the drafts do not make D17/D18 Accepted.
  Planning IDs D06/D17/D18 do not reserve ADR numbers.

### 6. Read-file SHA-256 ledger

These are the SHA-256 values computed live for every file used as a cited read
source above. Test-file paths in the inventory were mechanically enumerated and are
not asserted as individually reviewed source documents.

```text
5495d6e4147401e092bc557ac15aefad152e6f45f146f66b4f60d7d7225bfaa9  AGENTS.md
78bd4ef6b16a854b373d02d899a495a2a4e48deab608880de902118df248b66c  Cargo.toml
701e8556a138ce2081cc7484f404331fdc9160456ff4fbb46e78ce5e439b7417  Cargo.lock
c910997cb152c6dc8ed13b1fdf9fa28a4ed30d6776123e27dd6170e5c66067a0  rust-toolchain.toml
2d7516874f844580afa2e8f0a68d55681f820a268e181c8adf53401e09013fdb  docs/roadmap/m3-quality.md
22bb916b14aefcb63d266f4fa725538e4734f6ea6c6e1fecfcd35533907e69cb  docs/roadmap/m2-m8.md
9cf0387bbd68d76a5b7fcdeaee296fe03380cab67374e976ff9814070b27b2c0  docs/roadmap/adr-backlog-m2-m8.md
341401c7eb276119d06b41b8524a1fad547096f36ec0ca148c23530731db68b5  docs/prompts/implement-m3.md
30c8b7bace39dca66f024b914647a719732a50bf3c7966c10ded53515be9bbf9  docs/implementation-status.md
028a085af01f985572c70d4c7ca1bc6eb64ab32b770787e861bc073b84e8b60d  docs/validation/M2-07.md
f51a100a2d1f55183c7531389485aa4ace20e8ac5954e89e1c80d5bce9f5c612  docs/validation/M2-local-integration.json
35f21aeaaf0ec22a7eaf52707aaa6225603fab4630ba42b49419042d0e6915fc  docs/ci.md
c2953c5f372f8b9bdfc51860479ac7d5b8991be04bb8d745134fa838f3a78f92  docs/architecture.md
0521590e92771c7fc94b13e5c314ca9f4c65fe50206300c883f1381cf040bca8  docs/adr/ADR-028-ephemeral-artifact-store.md
9af5d5f64d8ae398604c05850e797edbe8c265e402969cebb03fbeaff8ab0001  docs/adr/ADR-030-m1-worker-admission.md
2fcd34c560d06940717c32449ac48e84d71f2c759e2faff97e330b582780dbf0  docs/adr/ADR-031-rust-source-transfer.md
d80688690c7e4a293f2a08f23bb269ae038335af3bdf454f61861ac9898a5d11  docs/adr/ADR-037-test-execution.md
66a28a89bc71e847e2443bb9b6fb39b0703fca3ff71b590941b944da8dd0fa44  docs/adr/ADR-040-single-capture-quality-gate.md
378ba4200ee0d224d27ef988bef47ac4394ac5104bf9bd6b839fa76776bee045  docs/adr/ADR-050-local-coordinated-mutation.md
296a431965f98c5c681688a28cc0b15fa0e9aa33f033942f635b06cb97c52b57  docs/adr/ADR-051-semantic-manifest-editor.md
ce39d4e64efbe324e58b874dce05bd2468860a21d98208a521a3f90802563058  docs/adr/ADR-052-mutation-journal-and-authorization.md
50c3252db875447f1f5f35e8d52a408995da6487bb0286160b4e952045da7b15  docs/adr/ADR-053-bounded-guest-mutation-staging.md
8e3400f7218153543d28caf50524c16c776a4199826833787cfdcbe160f0d763  docs/adr/ADR-054-multiple-file-mutation-publication.md
763a770bf53f39da67270e9463b591bf43d108c6fd39a751316e23af93da7bc0  docs/adr/ADR-055-offline-cargo-data-and-lock-policy.md
c80d2b7ed9b5d8454fd5ce93c6b39d5e1ca015a9987ea1919efc89405a8041b8  docs/adr/ADR-056-cargo-fix-isolated-loopback.md
8cbc71ac595453d461c14741a240cee6dc6bae53dff52266ebed6bd15379d504  docs/adr/ADR-057-typed-manifest-and-dependency-operations.md
1c5f1c7e114d01f15223be8e90e846c1ec1006ad09ae97be83e4d2f48a32b53f  docs/adr/ADR-058-local-mutation-observability.md
6e50817a794105ca271e46c7d75c089cf3c33c4360228bb6a32e7b730bdc9b84  docs/adr/ADR-059-terminal-plan-retirement-and-durable-replay.md
fa8fc5f7957737c84298c0fa2c367cfa895ff4b98f33bbeb38e7c3ba9f988802  docs/validation/m2-closure/verify-evidence.py
7b83c85dcddb32359a9fc207e09f87b3b0e8e6975727b3ae04b1f191131708ab  docs/validation/M2-image-config.json
05f0391041e053309e8924183600dcfbd8cd41b925afadb2edca7fcb73e43ea5  docs/validation/M2-full-gate.json
19a309cb133dc88c055e1c94888b0888ef02f36524877295de468e4c691b9952  docs/validation/m1-17-final-gate/receipt.json
035ddd2fea1635ff988b1a8283e13ee33d9d2bed77271a1c5c6ebd2a9604e303  fixtures/rust-runtime/README.md
44f0b9ad348b2972b0e63d85606df58e651279bf9ddb35e2ac65e3778a29edff  fixtures/rust-runtime/sources.json
fd3d253f80b76c39a537e8ada5464dde5c8dcf5835d6b307e808f228f4d59514  fixtures/rust-runtime/provision.py
e9dcf12764a5d06242566fa55b3ede2c1a89211e6e585bbe753cc1c97e53a5f7  fixtures/rust-runtime/verify.py
ee8599fb9f1683d3172e3a1d4a04a23270c125aab3303600b8dd800572fb5463  fixtures/rust-runtime/Dockerfile
a8b99ecd81ab4b24cf78ce90972b72e310fbd025ae3c13a2ca99168c37bc854a  crates/domain/src/rust_execution.rs
bb88a2fafa52f624f67d208897e3b27b8da9b993cccb0c7864195f7179b7f713  crates/application/src/execution.rs
e736e8f489d5360a0e9d2d5b815f450304a7ceea0740bd812819bb7baea6e483  crates/execution-adapter/src/rust_gateway.rs
f650bd956dd30e56929af7643265e49265d09be277e3039f775091caa022db19  crates/execution-adapter/src/supervisor.rs
8cf540a9ad1dad7602e15dfc30d65a26569dcbf163f9542e6c662c697da93f4d  crates/execution-adapter/src/project_inspection.rs
546e24b8c4c11d4c22c776bbcd234370f8b284906a3f23d7b9a2f56225a8e317  crates/mcp-server/src/stdio/workers.rs
3ba026113e2d0a77d71e558d2aec2d012ca13ca606df1323a0dad150febf1a3a  crates/mcp-server/src/stdio/admission.rs
16023ff0df4c0e37d9f364114f4c5b05606e583af56cd92f89e9e4d26da8efdd  crates/mcp-server/src/stdio/budget.rs
85058575c29a21aebee91611fbc0b8a5322f2fb90d2b8709f4f043d39e63d07d  crates/mcp-server/src/stdio/testing.rs
e8848842ac3db036b12f47cd47693d8ea3525183d838c40a877710863ce353a3  crates/mcp-server/src/stdio/quality.rs
81420de56b5c732335d46a342be79502d2704602d484dce36001cdb5c3b373cf  crates/mcp-server/src/stdio/project.rs
0c85bcf1e32502dda052ebbdbe7b101ac97c8240ab3280c20b93f10e37890d12  crates/mcp-server/src/stdio/resources.rs
e18e1f06d6353ad14ae72f8cf3d113637eb6604954c555b195c2ce818345a291  crates/mcp-server/src/stdio.rs
9118391523b4e6760be3e907fac7cb19885627c1028941074070e6e4be70c29b  crates/domain/src/artifact.rs
4d0c8fa749bd3ad46cfb64dfb49a2f888e019db303ae4fe8cf968ce8c620eda4  crates/application/src/artifact.rs
976917af526332b3fa8292a614cea8d560cd288553c16b08d8ad7ad330d520ab  crates/application/src/artifact_access.rs
e6df58aecb12fa7b1d53b9126ef5449e041cf1fc7e53204baafe84b31db58067  crates/artifact-adapter/src/lib.rs
66fb958d4bb5f870d153b377de3183b969275ff7fea61ec3f5fb325d088d4d08  crates/application/src/validation.rs
df7fe329bbcdbffc49cfdf461df9556041f1aae624c9bd0f47b4ece34b13db87  crates/execution-adapter/src/mutation_gateway.rs
146855b8e3377523ab15aab5a8a91d2b46bfc85a519c4471d12ac822f8ea7baf  crates/execution-adapter/src/applied.rs
c130cfee5f41be23ecf53ad65556db2d025f0ec8df56180ba2340ebf354bde97  crates/execution-adapter/src/rust_applied.rs
e8e7059558680df02ef41cb65af98a347b6cbbae452bda19ffb880c4fd5f9647  crates/execution-adapter/src/mutation_archive.rs
0dc7bbbbf8f7e2a47c784488c49bbc7ffd6acac83b6f15e4fe2f0b9c1f642f15  crates/execution-adapter/src/seccomp-rust-fix.json
be7b1ae1dacc0aa9b8fabafff6b226f7731d8019d8c069e07f97a972e4e394c5  crates/application/src/source.rs
d637e03d60df9a69a125c7b24a4e948e4d5918763586126235b1ff4468f20439  crates/application/src/inspection.rs
a49e55162b875fcc824dcb14e0f2cd95e793db7c960d8e7a4981f6a02412cfea  crates/application/src/lib.rs
d048b1bfcc16e81a54f4fab7588efe746b679872623a4a2ea0e83e2b5462b96d  crates/mcp-server/src/stdio/contract.rs
d0e2a13d1948d9cf787ebd9181089a3f1c7985a23cd0dbeb37c95e5e2f1f8bae  crates/mcp-server/tests/protocol.rs
b6bb173a199bd85919c1a8f97218c4f02162f106338abffef97e3a68b72269f2  crates/mcp-server/tests/snapshots/audit-tool.json
9ac8e2964beb0ae3af9829776761d25612ad4a57252dc19f98fa684f3c46c1bc  crates/mcp-server/tests/snapshots/catalog-status-tool.json
a9281b713e3f8b91682b0d248cacb7fc1e978b6d8df9baf0ad0f315a5a43f014  crates/mcp-server/tests/snapshots/check-tool.json
a4afb5c9d2c854ed23fb256c9ecdfd40e0ca5129a9aa302553a957e5346511a0  crates/mcp-server/tests/snapshots/clippy-tool.json
27af4b9f7c2df00fc2d528341ee781d350d09081bf2a7c9f3c5bc5538442d39d  crates/mcp-server/tests/snapshots/crate-inspect-tool.json
3d2a6d091a4a0ba0cfcdd1a4f8600e4cf4f883fc18ad4fee2412d4a1053f6039  crates/mcp-server/tests/snapshots/crate-search-tool.json
426515577746d42235ee31edfe455a5057cb0c7ffd1912f534de26e323304be6  crates/mcp-server/tests/snapshots/dependency-add-tool.json
e73dfb65c55c5de6852b77e947bb8ee79ce824f18ef3035b33f84836ab198cdd  crates/mcp-server/tests/snapshots/dependency-remove-tool.json
f4e0ab8b25a32e07ba6a86c100b7ee2a5adcab7a35f691f16e82c9d3f4933b9f  crates/mcp-server/tests/snapshots/explain-tool.json
89c93950abe47923644f1cb2457491fbf4823533b99a489a7723fc1786fd57d5  crates/mcp-server/tests/snapshots/fix-apply-tool.json
78983f1e4c08d607a4504994237a3a053de0f5c779671c0da6357db0bf833dab  crates/mcp-server/tests/snapshots/fmt-apply-tool.json
762e342a4b9e7e7e4a013583fb0f71ecefe1bf4c3ef47088b7c119a75dee56a4  crates/mcp-server/tests/snapshots/format-tool.json
1a4f9891093f03db987ad9b53f3b862a0dc54183abf074d89096f6f294b814aa  crates/mcp-server/tests/snapshots/manifest-patch-tool.json
bf0838ace4f0f7f0709c76801de6f1b66307d90251eab6441a2d07208b913e07  crates/mcp-server/tests/snapshots/project-inspect-tool.json
bfab409498dc67517a96d4d034f403d24ad3ae31046df3afc09c1f3781148d9b  crates/mcp-server/tests/snapshots/project-open-tool.json
95f2b8adf277a0d2d95b827a320c1f848bb1069482ad30d2e1592d418bdfffb3  crates/mcp-server/tests/snapshots/quality-tool.json
a4f0904c89b34bd4eb3524f9de71c8d4c2036028b5905bcfdc17eea75085abf7  crates/mcp-server/tests/snapshots/test-tool.json
bd57a8784387ac6939b3a12d2343660a1bf2c2fc1246c83790e6f7e0276ef518  crates/mcp-server/tests/snapshots/toolchain-inspect-tool.json
ccbf6a80b0d704412db91ff4a92521e73aee8db403345452bb34a08a703a7373  scripts/gate.py
8fae7e90beb9d0fa008142da1e71d5ced1a2695fde4df6671fb91fef1fc952bb  scripts/test-rust-execution.py
1639b7ed75665db8e98305f1c823fd5f6ff9a2d4e4123fefaa6656c21f21ca84  scripts/test-m2-runtime.py
f18397e4694521b62d6764c3baa8dc52188e8feb8ad09e1321a91e51ba1905cc  scripts/test-m2-clients.py
6b775fd2e4bb42ef4b052feee776f88c56a8819400b9fc95498aebe35d0789fa  scripts/codex-model-qualifier.py
d083a31142b5267366b02aa56b47b6cd12153936ec53ecf8e63e2c667e67da84  docs/tools.md
450566cffde0b2266ee436851541945a0e24ce954cc8edb721c5a12239d18e06  docs/validation/M2-matrix.md
d36121bc4a9dfaa1ffe7ec2a08409aec1b153a3e186f9932f153921474ee46c1  docs/adr/README.md
b5f439b550e7a40cd3bf770008dc7cdc7f427818fb623e671b3bf7f2c2f1cba0  docs/validation/m3-delegation/R01-plugins/stdout.txt
04ad62b3e37551422ff67e1902e372a8af6db2142dcd2aa76a71dfe82f1ea0cf  docs/validation/m3-delegation/S00b-branch/last-message.md
d24f80205d9403244e45cb80c9e62b1db589f847ed975d115c63cd260870c5f8  crates/mcp-server/tests/rmcp_tasks_spike.rs
25afe84d20ed52b830e9eec400802125db8d31cfb847d967a25abbda74324f0e  docs/adr/ADR-061-private-quality-artifact-store.md
d32f7917ad8075a69845eeb2cb4ac14860ed6d4077042b1cb04f3f7a9c358cf2  docs/adr/ADR-062-coverage-accounting-and-semver-baselines.md
b71a20c9d5682f720dda441493867f69d84b7be064ae54d7a719b4a876e77319  docs/validation/m3-delegation/R00-rmcp-tasks/report.md
4b6cf970ec507a582c87ed428d12e019f474d31f5b965206dc05642b558419a6  docs/validation/m3-delegation/R01-plugins/report.md
```

## Risks

1. **D06 is unresolved at the M3-01 deadline.**
   `docs/roadmap/adr-backlog-m2-m8.md:64-72` is still Proposed, while
   `m3-quality.md:25,43-60,134-135` requires a neutral JobExecutor, exact rmcp task
   spike, lifecycle/admission values, and provisioning to be decided before M3-01.
   There is no domain/application job state model or job persistence layer today.
2. **D17 is unresolved and the current ArtifactStore is structurally insufficient.**
   It is process-memory-only, opaque-byte-only, non-persistent, non-enumerable and
   limited to 256 KiB/artifact, 1 MiB/owner and 16 MiB global. M3 proposes rich
   descriptors, pagination/chunking, persistence/recovery and 32/64/128/256 MiB
   budgets. Raising current memory limits would violate D17's explicit warning.
3. **D18 remains Proposed.** M3-03/04 need compatible LLVM merge/run identity and
   two authorized SemVer snapshots with explicit incomplete semantics. Current
   two-ProjectRef capture can be composed, but there is no atomic cross-root snapshot
   or package/features/target compatibility contract.
4. **Provisioning is absent.** The immutable guest has none of the five required
   plugin/tool components. The host also lacks cargo-llvm-cov,
   cargo-semver-checks, llvm-profdata and llvm-cov; host cargo-nextest/mutants do not
   qualify the guest. cargo-mutants has no recorded upstream Linux ARM64 asset and
   therefore introduces a reproducible source-build/supply-chain decision.
5. **Current single-worker semantics cannot simply become task polling.** One
   fail-fast permit and no queue fit M1/M2. M3 requires state/poll/cancel not to
   acquire the running-job permit and cancellation to become terminal only after
   cleanup; that needs a separate owned job lifecycle while preserving the existing
   request/framing caps.
6. **Artifact publication is currently coupled to a live ProjectRef and one process.**
   Job/result expiry, owner revocation, restart behavior and on-disk recovery have no
   implementation. A task ID or artifact URI must never become authorization.
7. **M3-05 cannot use the ordinary read-only execution path.** The M2 staging
   primitive is safe to reuse, but its closed phases only support fmt/fix, its export
   is a whole-source USTAR, and its mutation scope accepts changed existing `.rs`
   files only. Mutant scheduling, baseline-before-mutation, bounded outputs/diffs and
   plugin-specific directories require typed extensions and recalibration.
8. **Gate drivers are static and source-bound.** Docker selections and stage lists
   are hard-coded. M3 additions must produce new receipts without rewriting the M2
   574-input closure or weakening exact one-test/cleanup assertions.
9. **Docker image presence is unknown in this delegate sandbox.** The socket path
   exists, but Docker API access was denied. No claim is made that the approved image
   is locally usable now.
10. **M2 full-gate environment provenance is incomplete.** The final receipt records
    the stage evidence but not the exact three environment path values; the ORT path
    comes from the earlier full-gate receipt and current files. Existing paths may be
    stale or altered and must be rehashed/requalified before M3 use.
11. **Concurrent orchestration changed branch state during inspection.** The final
    state is correct and documented, but future packages should establish exclusive
    ownership of Git state before concurrent delegates start. It also added an
    untracked rmcp spike test and Proposed ADR-061/062 drafts after S00 began; these
    are not part of the verified 574-input M2 baseline and have not been accepted.

## Decisions

- Treat S00 as a successful baseline inspection and branch end-state, but **do not
  authorize M3-01 implementation yet**.
- Preserve the current 18-tool contract and all M1/M2 snapshots until each M3 tool
  has a complete vertical and positive evidence; never advertise empty tools.
- Reuse the existing `RustCommand`/ExecutionPort/gateway/supervisor boundary, joined
  cleanup semantics, M2 private tmpfs mutation staging, strict USTAR export, and
  owner-bound Resource URI. Extend them through typed variants/ports after ADRs,
  not through arbitrary flags, shell, PATH discovery, or host-writable source.
- Reserve ADR-060 as the next numerically free ADR. Concurrent Proposed ADR-061 and
  ADR-062 drafts appear to target D17 and D18; the owner still needs to review and
  accept or reject them, and D06 still needs an accepted decision. Planning IDs do
  not predetermine ADR numbers.
- Keep exact plugin versions, assets, hashes and licenses in Proposed/TO-BE-VERIFIED
  state until the owner reviews official evidence and authorizes provisioning. A
  new image necessarily means a new immutable ID, amended ADR-031 evidence, new
  gateway fingerprints/calibration, and new qualification receipts.
- Keep M2 closure artifacts immutable. M3 gets separate validation matrices and
  receipts.

## Open issues

1. Owner authorization and accepted ADR(s) for D06 JobExecutor/task lifecycle and
   exact synchronous fallback values.
2. Owner authorization and accepted ADR for D17 persistent rich artifacts, storage
   location/permissions, recovery, migration, pagination, quotas and privacy.
3. Accepted D18 decision for coverage merge identity and two-ProjectRef SemVer
   baseline/candidate semantics before M3-03/04.
4. Independent verification and owner selection of exact guest plugin versions,
   release assets/source closure, SHA-256 values, attestations, licenses/notices and
   GNU-vs-musl policy; decide whether cargo-mutants source build is acceptable or
   M3-05 remains blocked.
5. Re-run the sole allowed Docker image inspect from an orchestrator context with
   socket permission to confirm the approved image ID exists locally before any
   calibration work.
6. Define a new M3 evidence/gate plan that preserves the 574-input M2 closure and
   adds exact positive/negative/absence/cancel/cleanup/client tests without dynamic
   discovery or silent skips.
