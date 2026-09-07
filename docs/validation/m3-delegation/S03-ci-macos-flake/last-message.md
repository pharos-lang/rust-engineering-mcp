Complete. `portable / aarch64-apple-darwin` passes.

## Task
Fix the two macOS-only CI failures on PR #14 (`filesystem::macos::mutation::tests::deterministic_pre_effect_interruptions_recover_without_source_write` and `…manifest_and_lock_first_swap_crash_rolls_forward_only_known_bytes`, both `Error: "Busy"`), then get the matrix green.

## Result
Fixed and pushed as `480ebd7`. The macOS job passes. The failure is reproducible on demand and the fix takes it from 13/200 to 0/200.

## Reproduction achieved
Nothing reproduced at 3/8/16/32 test threads (47 runs), nor across 6 concurrent processes (72 runs). It reproduced once I matched the *shape* of the CI window rather than its parallelism: the mutation subset at `--test-threads=8`, minus the two CPU-hog tests, against three concurrently spawning copies of the binary plus CPU load.

- Baseline: **13/200 runs failed**, 3 of them exactly `Error: "Busy"` on `deterministic_pre_effect_…`; the rest were `corrupt_store_is_quarantined_…` (8), `short_journal_writes_…`, `pending_replay_…`, `lost_temp_…` — all the same defect.
- Instrumented run captured it live: `flock` returned `EWOULDBLOCK` on `…/state/mutation-store.lock`, backtrace through `StateRoot::lock` → `NativeMutationStore::recover`, first statement.

## Root cause
`crates/project-adapter/tests/support/native_mutation.rs:743` and `:825` (pre-fix) call `Command::spawn()` from inside the parallel libtest harness.

`fork` copies the whole descriptor table, so a spawn on one thread keeps every `flock` a *sibling* thread holds alive inside the child until that child reaches `exec`. The sibling then closes its own descriptor, no longer owns the lock, and its next acquisition at `crates/project-adapter/src/filesystem/macos/mutation.rs:336` gets `EWOULDBLOCK`, which `mutation_io` (`mutation.rs:41`) maps to `Busy`. Both CI failures are the first statement of `recover` (`mutation.rs:1568`), and both occurred in the window where `killed_process_recovers_each_durable_boundary` and `killed_process_rolls_forward_known_format_prefix` — the only two tests here that spawn — were running (CI log 23:58:18.9–19.5).

The repo already documents this exact effect for the quality store (`tests/quality_artifact_store.rs:8-13`, ADR-061), where the spawning tests are `#[ignore]`d and gated at `--test-threads=1`. The M2 suite never applied that remedy.

Measured mechanism (C/Rust probes on this host):

| | spurious `EWOULDBLOCK` | excludes a sibling fd in-process |
|---|---|---|
| `flock(LOCK_EX\|LOCK_NB)` | 319,247 / 1,150,479 | yes |
| `fcntl(F_OFD_SETLK)` | 259,026 | yes |
| `fcntl(F_SETLK)` | **0** | **no** |

`POSIX_SPAWN_CLOEXEC_DEFAULT` does not help (198,540 vs 201,465); macOS 26's SDK publishes no `FD_CLOFORK`. Single-threaded probes showed 0/200 and 0/200 — the window is invisible unless a *different* thread releases during it, which is why my first two hypotheses were wrong.

Ruled out with evidence, not assumption: `fsync`/`F_FULLFSYNC` returning `EAGAIN` (0 in 26,483 calls), inode reuse / stale vnode locks (0 spurious in 10,491 unique-dir churns), `O_UNIQUE` locking semantics, and shared fixture state (paths are unique 128-bit randoms; the `lsof` holder was the same PID).

## The fix, and why it is the cause not the symptom
The two spawning tests now take a barrier no store-lock window overlaps: lock holders take its shared side inside `StateRoot::lock`, a spawn takes its exclusive side across `spawn()` itself. Only the *outermost* lock reads it — a commit holds the global and workspace lock at once, and a nested read would wait behind the spawn that is waiting for the outer read (I hit that deadlock and fixed it with a thread-local depth).

This removes the fork/lock overlap that *creates* the false `Busy`, rather than tolerating it. No serialization (concurrent lock holders still run concurrently), no sleeps, no retries, and the `Busy` contract is untouched — the barrier is `#[cfg(test)]`.

I did **not** swap the locking primitive: `global_lock_is_nonblocking_across_store_instances` (`tests/mutation_store.rs:896`) requires the store to conflict with a raw `flock` on a second descriptor in the same process, which `F_SETLK` cannot do. That would have relaxed the contract the package forbids relaxing.

## Files changed
```
3eaf333a5f9b06c128b4bb711b2d7ab578eeadea087afdc321196685f033c169  crates/project-adapter/src/filesystem/macos/mutation.rs
f6af9229a13594601992eb0e7817a8257493a119abf9817e97d4f1c94d207a5d  crates/project-adapter/tests/support/native_mutation.rs
```

## Tests executed
| | result |
|---|---|
| Repro harness, 200 runs, after fix | **0 failures** (baseline 13) |
| Both formerly failing tests together, `--test-threads=16`, ×40 | **40 pass / 0 fail** |
| `cargo test -p rust-engineering-project --locked --offline` | pass; `tests/mutation_store.rs` **20/20**; quality store 31 pass / 4 ignored |
| `cargo fmt --all --check` | pass |
| `cargo check --workspace --all-targets --locked --offline` | pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | pass |
| `cargo test --workspace --locked --offline` | exit 0, 84 `test result: ok` |
| `cargo test --workspace --all-targets --locked --offline` (CI's exact form) | exit 0, 77 `test result: ok` |
| `python3 -B scripts/check-architecture.py` | PASS |

## Final check matrix
`portable / aarch64-apple-darwin` **pass** (4m3s) · linux **pass** · windows **pass** · supply chain **pass** · SonarCloud **pass** · SonarCloud Code Analysis **pass** · CodeQL actions/python/rust **pass** · **CodeQL (GHAS alerts) fail** — see below. Nothing pending.

## Risks
- One green macOS run does not by itself prove a rare flake is gone. The evidence is 0/200 vs 13/200 on the harness that reproduced it.
- The barrier is test-only, so it protects the lib test binary and nothing else.

## Decisions
- Kept `flock`; kept the two tests running in PR CI rather than applying the ADR-061 `#[ignore]` + gate remedy, which would have removed coverage.
- Gated the barrier on `#[cfg(test)]`. Outside tests the only difference is that `StateRoot::lock` returns a private one-field wrapper around the same `OwnedFd` with the same drop. Product behaviour is unchanged, so **no receipt is stale and no gate needs re-running** — flagging it explicitly since it is technically a product file.

## Open issues
1. **`tests/mutation_store.rs` has the identical hazard, unfixed.** `global_lock_is_nonblocking_across_processes` spawns from inside its parallel harness while ~19 sibling tests take store locks. Never observed failing (1 spawn/run vs 10), and a `cfg(test)` barrier is unreachable from an integration target. Fixing it needs either a product-visible barrier or the ADR-061 remedy — your call.
2. **Production hazard, not addressed.** The MCP server spawns cargo/Docker subprocesses from worker threads in the same process as the mutation and quality stores, so a real `Busy` can be reported with no holder. `quality.rs` and `catalog_store.rs` use the same `flock` construction. ADR-061 already acknowledges this class; closing it properly is an ADR decision.
3. **CodeQL GHAS check fails, pre-existing and unrelated.** 33 alerts (31 critical), *all* in `crates/execution-adapter/*` — `rust_applied.rs` (17), `mutation_gateway.rs` (6), `rust_gateway.rs` (6), `mutation_outcomes.rs` (2), `resolution_gateway.rs` (2). Byte-identical count at `36c1d93` before my commit; none in the files I touched. Outside this package's scope but it is a real blocker on PR #14.
4. **Attribution format.** I could not determine this session's `claude.ai/code/session_…` URL, so `Claude-Session:` carries this session's local id `5ba136d4-a7ef-428f-bb3b-700f280a7e31` rather than the URL form earlier commits use.