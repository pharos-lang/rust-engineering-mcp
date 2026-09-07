## Task

Re-establish the M3 evidence on the final bytes of `ai/m3-quality`, assess the 33 CodeQL alerts, neutralize the personal username, commit/push, and get the required checks green. No merge, no approval.

## Result — per gate, on the final bytes

| Command | Exit | Stages | Duration | Receipt SHA-256 |
|---|---|---|---:|---|
| `check-architecture.py` | 0 | PASS | 0.104 s (in full) | console |
| `gate.py core` | 0 | **14/14** | 202.596 s | `7b59e852ea0b522b974fe3377503e813114b7dc573da506396cb70d54affd836` |
| `gate.py full` | 0 | **25/25** | 2,589.601 s | `035bfad65ffed692531d2a2fa049e370abfea2d6278cf27087f44e814d1f0e86` |

Counts: test 1,105 passed / 0 failed, 1 doctest, docker-security 4/4, rust-security **20/20**, m3-runtime **62/62**, semantic 17, catalog 6, catalog-status 3, crate-search 3, crate-inspect 2, python suites 9/11/9/39. Inventory **810 inputs / 46,027,636 bytes**, `source_inputs_unchanged: true` in both. The canonical `source_inputs` list hashes to `0af0ed9b…` in **both** runs — that is what proves the two gates qualified identical bytes.

Stage receipts landing elsewhere: `m3-runtime` and `rust-security` write into `target/`; both were copied to canonical names — `M3-runtime.json` `5cfab6c5…` (62/62), `M3-rust-security.json` `4ad812a0…` (20/20).

**Receipts preserved, not overwritten:** W6's four as `-preS03`; `M3-core-gate-attempt4.json` (core green on the tip *before* the username change — different bytes); `M3-core-gate-attempt5.json` and `M3-full-gate-attempt4.json` (the two `test`-stage failures, kept as-is).

## What changed since `e2ec7da`, and why re-running was necessary

Eight commits: `e07df9a`, `6b46baf`, `defbd7a`, `aa562c3`, `c2c0ea4`, `ea13d94`, `36c1d93`, `480ebd7` — portability fixes, the SonarCloud remediation refactors, and the fork/flock test-isolation fix. Most touched product code, so the `e2ec7da` receipts no longer described the bytes to be merged. I aborted my first full-gate run after ~1 minute on realizing the username change had to be *inside* the qualified bytes, not applied after.

## CodeQL assessment

31 `rust/hard-coded-cryptographic-value` are false positives: `nonce` here is the value of the Docker ownership label `org.rust-mcp.rust-job`, not cryptographic material.

**One correction to the brief:** not all 31 are test literals. **21** are inside `#[cfg(test)]`; the other **10** are in production fingerprint helpers (`rust_gateway::configuration_fingerprint` 6, `mutation_configuration_fingerprint` 2, `resolution_gateway::fingerprint` 2), where the literal is the explicit `"<nonce>"` sentinel used to build a deterministic template. Conclusion unchanged, scope corrected.

The 2 `rust/cleartext-logging` are `assert_eq!` failure messages in the redaction test. `("cburgosro", …)` → `("host-user", …)`; not in `GUEST_USERNAMES`, so the asserted `Redacted` property is unchanged. Recommendation recorded in the handoff: dismiss the 31 as false positives, and schedule the `nonce` rename as its own cut with its own gate.

## Two findings, reported rather than papered over

**W7-01 — pre-existing silent batch drop in rmcp 3.2.0.** The `test` stage failed twice on `batches_are_rejected_by_the_pinned_sdk_in_every_supported_mode`. On a legacy protocol version a batch draws `-32600` in 0.00 s *or* no response at all over 40 s, while the server stays healthy. The string comes from `rmcp-3.2.0/src/transport/async_rw.rs:182`. `Cargo.lock`, `Cargo.toml`, `vendor/` and `main.rs` are byte-identical to `e2ec7da`, and `e2ec7da` built in a worktree reproduces at the same rate — pre-existing, not from this branch. A green `test` stage does not prove it absent.

**W7-02 — one Linux failure, root cause NOT confirmed.** `cli.rs:131`, `Some(0)` vs `Some(1)`. Proven at compile level it isn't mine: a `compile_error!` in that `#[cfg(test)]` module doesn't fire for `--test cli` but does for execution-adapter's own test target. I first tried comparing binary SHA-256s, found the debug build isn't reproducible, and discarded the method instead of quoting it. Not reproduced (0/40 macOS); the fd-inheritance hypothesis was **refuted** (0/400 with and without concurrent forks). **I applied no fix** — fixing on an unconfirmed hypothesis would rewrite qualified bytes without grounds. It passed on the next run.

Note: an earlier CPU-load experiment of mine leaked 48 busy-loops (load avg 108); measurements taken in that window were redone on a quiet machine.

## Files changed (SHA-256)

```
ae379bbdd8c1a8f3f6af0c444a7aee595f9a25144cce5e8790406130734cefe2  crates/execution-adapter/src/mutation_outcomes.rs
20dcbce040b333bb283fb5f918fc92a0809561ede98dcc466fd70821458685b6  docs/validation/M3-07.md
255d754f84ffd15d49368087a07e2397511a044ff275228c67fa126b6273f09a  docs/validation/M3-matrix.md
d09619c2512ce6593fe37d87d3dd51bd27d9d692d5f8e4d0c4bbc1996411d059  docs/validation/M3-integration.json
7b59e852ea0b522b974fe3377503e813114b7dc573da506396cb70d54affd836  docs/validation/M3-core-gate.json
035bfad65ffed692531d2a2fa049e370abfea2d6278cf27087f44e814d1f0e86  docs/validation/M3-full-gate.json
5cfab6c56eaa9d6ab0f306b4920eb61eea21cd4cbc371bf7199c7094a674bf25  docs/validation/M3-runtime.json
4ad812a05b8683d049001d96b6f59871f1aa880fc865b93979dec58d5ff8df0b  docs/validation/M3-rust-security.json
60b313e99afd6550b9aaf30c872a3403c52475a804ce113bffa4dacf4651bc7b  docs/validation/M3-04.md
3a335382b2e6945f4c2f958bb90a56e7988a40a6fc6a183f0722454ed9c564cc  docs/validation/M3-05.md
e1a418e9def1ac632997754b46856bb264683c998dd88a670a9bea2981c4ebe6  docs/validation/M3-06-rollback.md
c5d9e048caf414628e727e8bca4e4329384f641c7707d89981767b0f0d3c4129  docs/implementation-status.md
```
Plus the 7 preserved receipts and the delegation logs (their `stdout.txt` are gitignored; the new files were scanned and carry nothing credential-shaped).

Commits `8a5fb7e` and `93991c4`, pushed. The second is docs-only, and the gate inventory excludes `docs/` — I verified the working tree's `source_inputs` still hashes to `0af0ed9b…`, so the receipts still describe the tree exactly.

## Final check matrix (tip `93991c4`)

| Check | Required | Result |
|---|---|---|
| portable / x86_64-unknown-linux-gnu | yes | **PASS** |
| portable / aarch64-apple-darwin | yes | **PASS** |
| portable / x86_64-pc-windows-msvc | yes | **PASS** |
| supply chain | yes | **PASS** |
| SonarCloud | yes | **PASS** |
| SonarCloud Code Analysis, CodeQL actions/python/rust | no | PASS |
| CodeQL (GHAS alerts) | no | FAIL — the 33 assessed alerts |

All five required checks green.

## Docker hygiene

`owned_containers=0`, `owned_volumes=0` (filter `label=org.rust-mcp.execution=true`) before the core gate, after the aborted full gate, after the core gate, and after the full gate.

## Risks

- A green `test` stage does not prove W7-01 absent; it can fail intermittently in CI and locally.
- W7-02 has one observation and no confirmed cause; it may recur.
- The full gate is a single sample per run; only the M3-runtime and rust-security stages carry their own selection-level receipts.

## Decisions

- Aborted the first full gate to put the username change inside the qualified bytes rather than after them.
- Did not fix W7-01 or W7-02: both are pre-existing or unconfirmed, and both would rewrite qualified execution paths. Recorded with reproductions instead.
- Left `cburgosro` in the three `#[ignore]` real-Docker socket-path fixtures and in historical receipts — outside the flagged scope, and rewriting past receipts would destroy evidence.
- Committed the delegation logs and the harness-incident note in `m3-delegation/README.md`, consistent with how other packages are tracked.

## Open issues

1. **W7-01** needs an SDK-level fix in its own cut with its own gate run.
2. **W7-02** needs an S03-style Linux reproduction before the oracle is touched.
3. The `nonce` → ownership-label rename is recommended for a later maintenance cut; the 31 alerts need dismissing in the Security tab.
4. `cburgosro` remains in `mutation_gateway.rs:1280`, `resolution_gateway.rs:1298`, `project_inspection.rs:1083` (ignored Docker fixtures) and several `scripts/*.py` — a separate neutralization pass if you want it.
5. Attribution uses this session's local id `024d2bde-…`; I could not determine the `claude.ai/code/session_…` URL form.
6. The merge remains blocked only by your review and CODEOWNERS.