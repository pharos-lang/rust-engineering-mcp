# M0-05 independent review

Claude Code 2.1.259; requested/primary model `claude-opus-5`, High; 340746 ms.
Read-only supplied packet; tools disabled, no edits or MCP access.
CLI telemetry also records ancillary Haiku usage; review content from Opus 5.
Packet SHA256: 85e4a088958afa5ef8c264ef091ad6b4737355f8ff8c87ef819156c594d6e75b

# M0-05 Execution Gateway — Independent Security Review

**Verdict: no P0. The foundation is architecturally sound and I found no way for the current package to execute attacker-chosen code, mount host state, or reach the network.** The argv path is closed (enums → fixed flags → validated hex image ID; no shell, no caller strings, no leading-dash injection). Cleanup precedes every success return, and cleanup failure is never laundered into `Ok`. The `slash()` runtime self-test (state.rs:47-68) is a genuinely good control: it proves the resolution flags behave as claimed and fails closed if they don't, rather than trusting a magic constant.

The defects below are concentrated in one theme: **the gateway asserts properties it does not verify, and records an identity that does not cover them.** That is harmless while nothing is announced, and unsafe the moment M0-06 binds capability claims to these outputs.

---

## P1

### 1. `execution_fingerprint` omits the container hardening configuration
`crates/execution-adapter/src/lib.rs:377-386` hashes engine identity, image ID, executable digest, scenario, limits, and `seccomp.json`. It does **not** cover `create_arguments` (lib.rs:257-299) — `--read-only`, `--network=none`, `--user=65532`, `--cap-drop=ALL`, `--no-new-privileges`, `--pids-limit`, `--cpus`, `--memory`, tmpfs options, `--entrypoint`.

**Concrete failure:** delete `--read-only` or raise `--pids-limit=64` to `--pids-limit=100000` and rebuild. Every fingerprint is byte-identical to the pre-change value. ADR-025:46 states capabilities are "tied to the exact engine/image/profile and proven by explicit M0-06 probes" — but a capability record keyed on this fingerprint would continue to validate against a materially weaker sandbox. This makes the M0-06 attestation unsound before M0-06 is written.

**Fix:** include the full, canonically-ordered `create_arguments` vector (with `name` and the state-dependent profile path elided or normalized) in the hashed tuple. Add a test asserting that mutating any hardening flag changes the fingerprint.

### 2. `admit_execution` cannot tell which configuration the capabilities were proven against
`crates/application/src/execution.rs:34-47` accepts a bare `SandboxCapabilities` with no binding to a fingerprint. A caller in M1 can pass capabilities measured on configuration A and execute on configuration B; the type system permits it and nothing detects it. This is the same defect as (1) surfaced at the admission boundary, and it is the function's entire purpose.

**Fix:** carry `ExecutionFingerprint` inside the capability record and require `admit_execution` to take the fingerprint of the configuration about to run, denying on mismatch. Doing this now costs one field and closes the hole before any caller exists.

---

## P2

### 3. `restart_syscall` is absent from the seccomp allowlist
`crates/execution-adapter/src/seccomp.json:12-123`. `restart_syscall` is inserted by the kernel — not by the program — when a signal interrupts a restartable syscall (`clock_nanosleep`, `futex`, `ppoll`, all allowed here). With `defaultAction: SCMP_ACT_ERRNO / errno 1`, the restart returns **EPERM** to a caller that cannot have made the call.

**Concrete failure:** the Go probe's runtime delivers SIGURG preemption signals continuously. The `Sleep` and `Descendants` scenarios are exactly the ones that sit in interruptible sleeps, so they are the most exposed. Expect intermittent spurious sleep failures — flaky `timeout_cancellation_and_stream_limits_clean_up_the_container` (tests/gateway.rs:96) and, worse, **false-negative wall-time and descendant capability results in M0-06**, which under ADR-025:50 ("missing, failed or unverified guarantees cause rejection") would reject a correctly-hardened engine. Docker's own default profile allowlists `restart_syscall`.

**Fix:** add `restart_syscall` to the allowlist. While there, add `mremap` (glibc `realloc` on large blocks — required before any M1 Rust image) and `clock_getres`.

### 4. No verification that the daemon applied the requested restrictions
After `container create` succeeds (lib.rs:325-330) the gateway never inspects the created container's `HostConfig`/`Config`. `docker info` gating (lib.rs:128-141) covers memory/swap/CPU-quota/PIDs accounting and seccomp presence, which is good, but it does not confirm that *this container* received `ReadonlyRootfs`, `NetworkMode=none`, `User=65532:65532`, `CapDrop=[ALL]`, `PidsLimit=64`, `NanoCpus`, the seccomp profile, or a private PID namespace. The PID-namespace guarantee in ADR-025:32 rests entirely on an unstated CLI default (`--pid` is never passed).

**Concrete failure:** CLI/daemon version skew or a future flag rename silently drops a flag; create still succeeds; the run is reported as a normal `Exited` result and M0-06 would measure an unhardened container.

**Fix:** after create, `container inspect` and assert the applied fields (including `HostConfig.PidMode == ""`), returning `InvalidConfiguration` on any mismatch. Note `--pid=private` is *not* a valid Docker value — assert via inspect, do not add the flag.

### 5. Orphan containers survive process death; the label control is dead code
`--label org.rust-mcp.execution=true` (lib.rs:263-264) is written and **never read anywhere**. `quarantined` (lib.rs:91) is in-memory only. `DockerGateway::new` performs no reconciliation.

**Concrete failure:** the host process is SIGKILLed mid-run. The container — e.g. a `Sleep` or `Descendants` container with no internal deadline — keeps running indefinitely; nothing removes it. The stale `rust-mcp-control-*` state directory also leaks with its seccomp profile intact. A fresh `DockerGateway` starts unquarantined and reports healthy, contradicting ADR-025:41-42, which treats uncertain cleanup as a gateway-disabling condition.

**Fix:** in `new()`, `container ls --all --filter label=org.rust-mcp.execution=true`; if any exist, refuse to start (or remove and record the event) rather than starting clean. Persist the quarantine flag as a file in the state root and honor it on construction. Sweep stale `rust-mcp-control-*` directories under the authorized root.

### 6. `duration_ms` measures gateway overhead, not the sandboxed run
`started` is taken before container create (lib.rs:322); `duration_ms` is computed after `remove()` (lib.rs:397). Every result therefore includes create, start, inspect and force-remove latency.

**Concrete failure:** a run with `wall_ms = 700` that correctly times out reports `duration_ms` of roughly 1500-3000 on Docker Desktop. Alongside `termination: TimedOut` this reads as the wall limit not being enforced, and any M0-06 probe that checks `duration_ms <= wall_ms` to prove `wall_time_limited` will fail against a working sandbox.

**Fix:** return `started.elapsed()` from `supervisor::run` in `Capture` (measured from spawn at supervisor.rs:89) and report that; keep total wall time as a separate field if useful.

### 7. State root ownership and permissions are never validated
`State::new` (state.rs:77-118) verifies the filesystem is APFS and creates a 0700 directory, but never checks that `root` is owned by the effective uid or that it is not group/world-writable. `check()` (state.rs:136-153) validates dev/ino of root and control directory by fd — but the Docker CLI consumes `--config <path>` and `--security-opt=seccomp=<path>` **by path string** (lib.rs:199-211, 248-251, 296), resolved after `check()` returns. The no-follow/beneath guarantee described in ADR-025:19-20 protects the gateway's own opens only, not the CLI's.

**Concrete failure:** with a shared or misconfigured `state_root`, the interval between `check()` and the CLI's own `open()` is exploitable to substitute a permissive `seccomp.json`. The fingerprint would still report the compiled-in strict profile (it hashes `include_str!`, not the on-disk bytes).

I accept the brief's statement that the state root is trusted TCB — so this is P2, not P1 — but the ADR currently overstates what is enforced.

**Fix:** `fstat` the root fd in `State::new` and require `st_uid == geteuid()` and `st_mode & 0o022 == 0`. Re-read and compare `seccomp.json` bytes against `include_bytes!` immediately before create. Amend ADR-025 to say the control-file guarantee covers gateway opens, with CLI path consumption a residual reliance on the trusted state root.

### 8. `SandboxCapabilities::satisfies(SandboxTier::None)` returns `true`
`crates/domain/src/execution.rs:28-30`. The weakest tier is satisfied by `SandboxCapabilities::default()` — all guarantees false. Today `admit_execution` independently rejects `None` (application/src/execution.rs:43), so there is no live bypass; but the domain primitive is inverted relative to ADR-025:49 ("`none` never permits project code"), and any future caller reaching for `satisfies` directly inherits an unconditional pass.

**Fix:** return `false` for `None` (or drop `None` from the tier enum and represent absence as `Option<SandboxTier>`). Add the case to `crates/application/tests/execution.rs`.

---

## P3

9. **`Denied` conflates policy refusal with gateway-busy and mutex poisoning** (lib.rs:309). `try_lock` returns `Err` both when another execution is in flight and when a previous execution panicked while holding the lock; both map to the same code used for admission denial. A poisoned lock also leaves the in-flight container unremoved and the gateway *not* quarantined — it just returns `Denied` forever. Use a distinct `Busy` variant and treat poisoning as quarantine-worthy.

10. **`executable_digest` is computed once and never re-verified** (lib.rs:116); execution resolves `config.executable` by path each time (lib.rs:198). The digest in the fingerprint therefore attests to the binary present at construction, not the one executed. Acceptable under the stated TCB, but the fingerprint implies more than it delivers — say so in ADR-025.

11. **`defaultErrnoRet: 1` (EPERM) breaks probe-and-fallback code paths** (seccomp.json:3). `clone3` is correctly given ENOSYS (38), but `openat2`, `pidfd_open`, `membarrier` and similar feature probes receive EPERM, which some runtimes treat as a hard error rather than "unavailable." Prefer ENOSYS as the default and reserve EPERM for syscalls you intend to deny loudly.

12. **~12 allowlist entries do not exist on aarch64** (`mkdir`, `rmdir`, `unlink`, `rename`, `symlink`, `link`, `readlink`, `chmod`, `dup2`, `pipe`, `truncate`, `getrlimit`). libseccomp skips unresolvable names, so behavior is correct, but the list overstates coverage and invites the wrong conclusion on review. Remove them or comment the intent.

13. **`/work` is `nosuid,nodev` but not `noexec`, unlike `/tmp`** (lib.rs:282-284). Almost certainly deliberate (M1 needs to exec build artifacts), but the asymmetry is undocumented; ADR-025:34 mentions only "explicit tmpfs sizes." State the exec allowance and its rationale.

14. **Reader threads can outlive `run`** (supervisor.rs:121-124). A blocked read yields `Infrastructure` after 2s while the thread and its pipe fd leak; `Infrastructure` does not quarantine, so repeated occurrences accumulate. Bounded per-occurrence, unbounded over time.

15. **`st_nlink == 1` and NOFOLLOW_ANY reject legitimate installs** (state.rs:176-181) — any symlinked or hardlinked `docker` (Homebrew, `/usr/local/bin`) yields `InvalidConfiguration`, indistinguishable from an attack. Fail-closed is right; add a distinct diagnostic.

16. **Bare errno `107` in `slash()`** (state.rs:59) with no named constant. Correct fail-closed behavior if a future macOS changes it, but unreviewable as written.

---

## Confirmed as declared, not defects

`admit_execution` unused in this milestone; no capabilities announced; no Cargo/arbitrary programs; no host or project mounts; no MCP surface; non-macOS fails closed (state.rs:204-221); daemon/host failure outside synchronous cleanup returning `CleanupUncertain` + quarantine (lib.rs:230-242). The `clone` mask `0x7E020000` correctly denies all seven namespace-creation flags, and `clone3` → ENOSYS correctly forces the filtered path. Socket, mount, `unshare`/`setns`, `ptrace`, `bpf` and `keyctl` are all absent from the allowlist, matching ADR-025:35-37.

## Readiness

**M0-05 is ready to merge once P1-1, P1-2 and P2-3 are fixed.** Those three are cheap and must land before M0-06 begins, because M0-06 records capability facts against a fingerprint that currently cannot detect a weakened sandbox, and its wall-time/descendant probes will be measured through a seccomp profile that EPERMs kernel-inserted restarts. P2-4 through P2-8 should land in the same milestone; P3 items can be scheduled. Nothing here makes the current, unannounced foundation unsafe to run against the trusted probe image.

## Follow-up Opus 5 Medium — 139584 ms

## Closure verdicts

**P1-1 (fingerprint omits hardening argv) — closed.** `configuration_fingerprint()` now generates the full argument vector for all 13 scenarios (covering the `Pids` 256m memory divergence) and hashes it with the engine record, client digest and embedded seccomp content. The unit test deletes each argument and asserts divergence. Runtime-varying tokens are correctly stubbed (`<container>`, `<seccomp-profile>`) while the profile *content* is bound via `include_str!`, and the image ID is inside the argv, so it is bound transitively.

**P1-2 (unbound capabilities) — closed.** `admit_execution` compares `evidence.configuration_fingerprint` against the caller's expected value before any tier logic, and `capabilities_from_another_configuration_never_authorize_execution` pins it. Ordering is right: mismatch denies before the capability bitmap is read.

**P2-3 (restart_syscall) — not verifiable here.** `src/seccomp.json` was not in the supplied set; I can only confirm the profile is content-bound at fingerprint time and re-compared against the daemon-embedded copy in `applied::verify`. Please treat this closure as asserted, not reviewed.

## P1

**P1-A — a container that never started is reported as a clean exit 0.**
`lib.rs`, exit-code branch: when `outcome.stop == Stop::Exited` the docker CLI's own exit status (`outcome.code`) is discarded, and acceptance requires only `!running && pid == 0`. A container whose `start` was rejected by the daemon (seccomp load failure, runtime error, OOM of the VM) stays in state `created` with `Running=false`, `Pid=0`, `ExitCode=0`. The gateway then returns `Exited`/`Some(0)` with the CLI's error text in `stderr`. For M0-06 this is the dangerous direction: `Success`, and every scenario whose expected outcome is exit 0, would be satisfied by a container that never executed, converting an infrastructure failure into positive capability evidence.
Fix: deserialize `State.Status` and require `"exited"`, and require `outcome.code == Some(0)`; treat anything else as `Infrastructure`.

**P1-B — neither the engine record nor the applied config pins the container runtime.**
`EngineIdentity` captures cgroup version, seccomp presence and the four limit booleans but not `DefaultRuntime`; `applied::verify` does not read `HostConfig.Runtime`. `--runtime` is never passed, so the daemon default applies. A host whose `daemon.json` sets a non-runc `default-runtime` still reports `name=seccomp` in `SecurityOptions`, still accepts every generated flag, and still yields an applied config that passes all current assertions — while the guarantees those flags encode may be enforced differently or not at all. The configuration fingerprint would also be unchanged, so admission cannot distinguish the two hosts.
Fix: add `DefaultRuntime` to `EngineIdentity` (it appears in `docker info --format '{{json .}}'`, so it also strengthens the fingerprint for free) and assert `HostConfig.Runtime == "runc"` in `verify`.

## P2

**P2-A — applied verification is an allowlist over a non-exhaustive struct.** `Created`/`HostConfig` have no `deny_unknown_fields` (correctly — Docker adds fields), so any applied setting not enumerated is invisible. The daemon-default-settable ones that matter: `Init` (daemon `"init": true` bind-mounts `docker-init` and changes PID 1), `MaskedPaths`/`ReadonlyPaths` (a daemon with non-default masking exposes `/proc/kcore`), `UsernsMode`, `CgroupParent`, `Sysctls`, and `default-ulimits`. Suggest asserting `Init` is false/null and that `MaskedPaths`/`ReadonlyPaths` equal the expected defaults before M0-06 turns this into evidence.

**P2-B — the gateway label is never verified as applied.** Startup orphan detection depends entirely on `label=org.rust-mcp.execution=true`, but `Created`/`Config` does not deserialize `Labels`, so `verify` does not confirm the label landed. If it were ever dropped, a leaked container becomes invisible to the next `DockerGateway::new` and the conservative startup gate silently degrades. One-line addition to `verify`.

**P2-C — the output budget is per-stream, so the aggregate bound is 2×.** Reader threads cap each of stdout/stderr at `limit`, `bounded_text` re-applies `limit` per stream, and `ExecutionLimits::output_bytes` reads as a single ceiling. Peak retained bytes are `2 * output_bytes` (plus lossy-UTF-8 expansion before truncation, bounded at 3×). Behaviourally fine and fail-closed; it is the `output_limited` capability wording and the type's doc comment that will overstate it in M0-06. Rename or document as per-stream.

**P2-D — cross-instance exclusion is unreviewed.** `busy`/`quarantined` are per-`DockerGateway` fields, so the single-execution invariant holds only if `State::new` takes an exclusive lock on `state_root`. `state.rs` was not supplied; if it does not, two gateways over the same state root each pass the startup label check and can interleave. Flagging for confirmation only.

Nonblocking readers: no regression found. Drop order (`readers` before `child`) plus nonblocking pipes and the cooperative `stop` flag means joins terminate within one 5 ms poll even with the child alive; `sync_channel(2)` cannot block either sender; the post-drain `Exited`→`OutputLimit` correction closes the fast-exit race.
