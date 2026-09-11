# M1-01 Rust gateway — independent security review

Claude Code2.1.259; explicit claude-opus-5, effort high. Read-only packet, no tools,
restricted/safe mode, strict MCP config, no permission prompts or session storage.
Actual modelUsage confirms Opus5; auxiliary Haiku telemetry is recorded by the CLI,
not a replacement review. Source packet and raw JSON remain under target/.

## Principal disposition

| Finding | Disposition and evidence |
| --- | --- |
| P1-1 errors masked by cancellation | Fixed: finish_work propagates every Err after cleanup. Regression covers all seven ExecutionError variants with cancellation/deadline/none; real six-scenario calibration rerun. A terminal label cannot stand in for a harness error. |
| P2-1 fingerprint implementation/limits | Strengthened with embedded verifier, source bounds, archive, supervisor, calibration, state, lock and toolchain implementation digest. A JSON receipt never was accepted as authorization: only active private instance state admits execution. Shared constants alone would not fingerprint changed predicates. |
| P2-2 OOM evidence | Fixed: completed-container OOMKilled propagates alongside Capture; incomplete/stopped observation remains nullable. Regression preserves observed OOM independently of interruption status. |
| P2-3 denied syscall/FD evidence | Added12 invalid-argument syscall controls and bounded /proc/self/fd socket observation after closing positive IPC pairs. Both real build.rs and invoked proc macro pass. For privileged calls EPERM can also come from kernel/caps, so attribution is not exclusively seccomp; applied profile bytes independently enforce the deny list. |
| P2-4 exclusive executable mount claim | Rejected as stated: ADR never promised /work was the only rw+exec Docker mount. Added actual /dev/shm rw/noexec check and explicit scope. The approved requested/applied mount policy remains exact; no host source bind is admitted. |
| P2-5 admission fingerprint | Fixed: execution identity includes project versus calibration scope. No calibration report can be supplied as project authority. |
| P3-1 startup reconciliation wording | Corrected: refuses labelled leftovers; explicit host recovery, no automatic deletion of unknown work. |
| P3-2 prlimit64/rlimits | Documented: cgroups/tmpfs are calibrated bounds; per-process rlimit defaults are not pinned or claimed. Raising a soft limit cannot remove cgroup quotas. |
| P3-3 tar umask | Clarified archive modes versus potentially more restrictive extracted modes. Fixed trusted runtime and actual Cargo test verify readability; restrictive umask fails closed. |
| P3-4 archive names | No change: encode accepts only owned SourceBundle with private fields/validated constructors; it is not an archive parser. Source limits/path validation are now also in implementation identity. ADR explicitly relies on the domain boundary. |
| P3-5 patch/replace | Clarified: validated patch dependencies; legacy replace rejected. manifest.rs already enforces both, with tests. No path-validator bypass identified. |
| P3-6 whole-operation wall budget | Intentional: includes preflight/transfer/Cargo. Pre-start timeout is valid; calibration requires a live independent descendant witness before interruption can count as cleanup evidence. |
| P3-7 Denied vocabulary | Retained for internal policy/budget rejection. No new public tool or public error semantics in this unit. Future MCP mapping remains explicit. |
| P3-8 ioctl | Residual documented: guest private devices, no host descriptors or controlling tty; no host-terminal isolation claim derives from syscall filtering alone. No evidence justifies broadening permissions. |
| P3-9 omitted applied fields | Added Tty/OpenStdin/AttachStdin/StdinOnce, AutoRemove, GroupAdd, UTSMode, OomKillDisable/OomScoreAdj, DeviceCgroupRules, StorageOpt and Annotations checks; relevant mutations rejected. Unknown daemon metadata is not claimed as exhaustive verification. |
| P3-10 timing window | Fail-closed calibration by design. Missed independent observation rejects capability rather than pretending the descendant existed. Current actual repeated gates pass; no arbitrary-load timing guarantee. |

Principal accepts the bounded prerequisite after these fixes, passing focused tests,
actual Rust calibration and final core gate. MCP integration is a separate vertical
requirement; project.inspect remains In progress. No native-platform/distribution
or general third-party crate compatibility claim is added.

## Original review

## Summary

I read the ADR, the four new adapter modules, the seccomp profile, the fixtures, and the supplied receipt against the base gateway/supervisor/state code. The invocation surface is genuinely closed, the applied-config verifier is unusually thorough, the seccomp delta is correctly constructed, and the guest-side evidence for cgroups, mount modes and process credentials is real. **No P0.** One P1 (evidence/termination integrity), five P2s, nine P3s.

---

## P1

### P1-1 — Deadline/cancel rewrites hard errors into benign terminations, including the two observations that gate `verified`

**`crates/execution-adapter/src/rust_gateway.rs:667-679`**

```rust
let terminal_signal = budget.stop();
self.cleanup(...)?;
let outcome = match (work, terminal_signal) {
    ...
    (Err(_), Some(stop)) => budget.stopped_capture(stop),
```

`(Err(_), Some(stop))` discards the error *value* and synthesizes a clean capture. Errors reachable inside the `work` closure include `InvalidConfiguration` from `rust_applied::verify` (line 498 — the applied-config check on the container that runs untrusted code), `Infrastructure` from the post-exit `ContainerState::completed` consistency check (line 523), and `CleanupUncertain` from `remove(&ingest)` (line 654) and every `absent()` call. The final `cleanup(...)?` on line 668 runs first, so a *later* successful cleanup is enough to let the earlier error be rewritten.

Failure path: `budget.stop()` is `Some` whenever the wall deadline has elapsed or the latch is set — and that is attacker/caller-influenceable, since untrusted build-script code controls when the job finishes and the caller controls `wall_ms`. The result is `Ok(ExecutionResult { termination: TimedOut, exit_code: None, .. })` carrying a normal execution fingerprint.

The sharp consequence is in calibration, not project runs. `rust_calibration.rs:403-432` accepts the `timeout` and `cancel` observations on exactly `execution.termination == expected && exit_code.is_none()`. For those two scenarios `interrupted_calibration` (line 287-295) sets a 5 s wall and deliberately drives the budget past it, so `terminal_signal` is essentially always `Some`. A masked internal failure produces precisely the capture the assertion expects. Two of the six observations that gate `set_verified(true)` (line 454) and that are published as `detached_processes` evidence therefore cannot distinguish "the guest was contained and killed at the deadline" from "the harness failed and the failure was relabelled". The same path can set `quarantined` (via `remove`) while still returning a report with `verified: true`.

**Fix:** never synthesize a capture from an `Err`. Apply `terminal_signal` only to the `Ok` arm and propagate `Err` unconditionally; if any error must be tolerated, allowlist it explicitly and never include `CleanupUncertain`, `InvalidConfiguration` or `Denied`. Additionally, record in `RustCalibrationObservation` which code path produced the termination, so the receipt distinguishes supervisor-observed termination from a rewritten one.

---

## P2

### P2-1 — Fingerprint does not cover the verifier or the transfer limits it attests to

**`rust_gateway.rs:240-279`, `rust_applied.rs:102-176`, `source_archive.rs:4`, `supervisor.rs:68`**

`configuration_fingerprint` hashes argv, image ID, engine identity, docker-binary digest, the seccomp bytes and a manual `"rust-source-profile-v1"` tag. It does **not** hash the expectations that actually enforce that argv — the constants in `rust_applied::verify` (masked/readonly path lists, tmpfs option strings, numeric limits, mount predicates) — nor `MAX_ARCHIVE`/`MAX_INPUT_BYTES`/the SourceBundle limits. Deleting `&& !h.privileged` or `&& h.readonly_rootfs`, or raising the per-file source limit, leaves every configuration and execution fingerprint byte-identical. A stored calibration receipt then keeps attesting to a configuration that is no longer in force; the only control is remembering to bump the version string by hand.

**Fix:** hoist the expected values into one shared const structure consumed by both `arguments()` and `verify()`, serialize it into the fingerprint tuple, and include the archive/source limit constants.

### P2-2 — `oom_killed` is hard-coded `None`, discarding data already fetched

**`rust_gateway.rs:707`, vs `lib.rs:97-98, 525-526`**

`phase()` already inspects the container and deserializes `ContainerState`, which carries `OOMKilled`; M0 reports it. Here it is dropped. If `rustc`/`cargo` itself is killed by the 1 GiB memory cgroup, the result is `termination: exited, exit_code: 137, oom_killed: null` — indistinguishable from a compilation failure. The resources fixture proves OOM enforcement for a *child* it spawns; nothing reports it for the compiler.

**Fix:** return `containers[0].state.oom_killed` from `phase()` and propagate it.

### P2-3 — Denied-syscall claims in the ADR are asserted, never exercised

**`fixtures/security/rust-containment/checks.rs:64-144` vs `ADR-031:82-88`**

The fixture proves `socket()` denial and the exact `socketpair` allow-shape (good, including the `SOCK_TYPE_MASK` flag cases). It never touches `bind`, `connect`, `listen`, `unshare`, `setns`, `mount`, `ptrace`, `mknodat`, `keyctl`, `bpf`, `io_uring_setup`, or `clone(CLONE_NEWUSER)` — all of which the ADR claims are denied. These are testable without valid arguments: `SCMP_ACT_ERRNO` fires before argument validation, so `connect(-1, NULL, 0)` returning `EPERM` (rather than `EBADF`) is positive proof of denial, and `unshare(CLONE_NEWUSER)` returning `EPERM` likewise. As written, a future edit re-adding the `socket` family or `unshare` still passes calibration. The ADR's "no inherited network descriptors" (line 85) is also untested.

**Fix:** add a denied-syscall table to `checks.rs` asserting `EPERM` for each named class, plus a `/proc/self/fd` enumeration asserting the fd set is exactly stdio plus the pair created in-test and contains no other `socket:[...]`.

### P2-4 — Mount evidence is per-path, not exhaustive; "only /work is exec" is unproven

**`checks.rs:187-220`**

`mount_options` asserts *exactly one* entry for each of `/`, `/source`, `/work`, `/tmp`, which is good, but nothing enumerates the complete `/proc/self/mountinfo` set. The exec boundary the ADR relies on (lines 68-70, 119-121: `/work` explicitly exec, everything else not) is therefore not established — an additional `rw`+exec mount, or a change in Docker's `/dev/shm` defaults (`--shm-size=1m` is set but its options are never checked host- or guest-side), would pass.

**Fix:** assert the full mountpoint set against an expected list and assert that `/work` is the only entry that is both `rw` and not `noexec`.

### P2-5 — Execution fingerprint does not encode the admission path

**`rust_gateway.rs:692-699` vs `lib.rs:537-542`**

M0 includes the `Profile` discriminator in the execution identity. Here `Admission::Project` and `Admission::Calibration` produce structurally identical fingerprints, so the `ExecutionResult`s embedded in `RustCalibrationReport.observations` (which ran *without* `verified`, by design) are, by fingerprint alone, indistinguishable from project executions. Only the archive digest differs, and only to someone who already knows the fixture digests.

**Fix:** add the admission/scope discriminator to the identity tuple at line 692.

---

## P3

1. **ADR overclaims startup behaviour.** `ADR-031:77` says startup "reconciles labelled containers and volumes"; `rust_gateway.rs:173-181` and `lib.rs:218-233` only refuse construction with `CleanupUncertain`. After a crash the gateway is permanently unconstructible until manual cleanup. Fix the text or implement own-label reclamation.
2. **`prlimit64` is a setter.** `seccomp-rust.json:52` allows it while denying `setrlimit`; a guest can raise its soft limits to the hard limits. Combined with `rust_applied.rs:107` verifying `Ulimits` *empty*, the effective rlimits are unpinned daemon defaults. Pin explicit `--ulimit` values and verify them, or state the reliance on the pids/memory cgroups.
3. **`--no-same-permissions` makes extracted modes umask-dependent.** `rust_gateway.rs:38`; `ADR-031:57-58` asserts 0755/0444. Root tar defaults to `-p`, which would give exactly the archive modes deterministically. A nonstandard daemon umask silently makes `/source` unreadable to uid 65534.
4. **Encoder does not validate names before a UID-0 extractor.** `source_archive.rs:13-27` checks only `name.len() <= 100`; leading `/` and `..` components are rejected only by the domain type, by GNU tar's own behaviour, and by the read-only rootfs — none of which the ADR names as a relied-upon control. Add the check in `entry()`.
5. **ADR rejection list omits `[patch]`/`[replace]` path keys.** `ADR-031:35-40` claims "reject external or absolute Cargo filesystem paths" but enumerates only dependencies/targets/build scripts/package+workspace metadata. Effects are confined to the container rootfs and `--frozen` makes them fail, but the list should match the claim (or the validator should be cited as covering them).
6. **Wall budget starts before host setup.** `rust_gateway.rs:565, 601-606`: the docker-binary re-digest (up to 128 MiB, `state.rs:174-208`), `docker info`, and archive encoding all consume it. A legitimate small `wall_ms` can return `timed_out` with no output and no container ever created, indistinguishable from a guest timeout.
7. **`Denied` is overloaded.** `source_archive.rs:8, 21` returns `Denied` for oversize archives; `rust_gateway.rs:538, 583` returns `Denied` for "not calibrated". Callers cannot distinguish policy rejection from capability denial.
8. **`ioctl` unfiltered.** `seccomp-rust.json:29`, where Docker's default denies `TIOCSTI`/`TIOCLINUX`. Currently unreachable (no `--tty`, so `/dev/tty` open fails without a controlling terminal), but the parity gap is free to close.
9. **`verify` is an allowlist, not a total check.** `rust_applied.rs:27-64` deliberately omits `deny_unknown_fields`; `AutoRemove`, `GroupAdd`, `UTSMode`, `OomScoreAdj`, `StorageOpt`, `DeviceCgroupRules`, `Annotations` and `Config.Tty`/`OpenStdin` are unchecked. None are exploitable here (`mknodat` is denied so device nodes cannot be created; no tty is allocated), but `ADR-031:73`'s "Each applied container and volume configuration is verified" overstates it. Reword or extend.
10. **Calibration observation is timing-flaky.** `rust_calibration.rs:306-331` polls at 20 ms against a 4 s deadline; the overflow fixture gives only a 1 s window (`build_overflow.rs:9`) before flooding. A loaded host misses the observation, the latch fires, termination becomes `Cancelled` instead of `OutputLimit`, and `calibrate` returns `Denied`. Fail-closed but spuriously.

---

## Areas assessed sound

- **Closed invocation.** No caller bytes reach argv. The only variable components are the hex nonce from `state::nonce()`, names derived from it, and the state-dir seccomp path (constrained by `state::valid_path`). `--entrypoint` plus fixed `Cmd`, no shell, and `rust_applied` re-checks `Entrypoint`/`Cmd`/`Env` by exact equality.
- **Applied configuration.** Image referenced and verified by content digest; `Env` exact-set equality (closing the image-env leak); masked/readonly path lists, mount request *and* applied shapes, `NoCopy`, `Subpath`, driver options, `RW`/`ReadOnly` both directions, and byte-equality of the applied seccomp JSON against the embedded profile — that last one also defeats tampering with the on-disk profile that `State::check()` does not cover.
- **Seccomp delta.** `clone` mask `0x7E020000` correctly denies all namespace flags; `clone3 → ENOSYS` is the right construction; `socketpair` arg filter uses `(type & 0xF) == SOCK_SEQPACKET`, correctly tolerating `SOCK_NONBLOCK`/`SOCK_CLOEXEC`; unconditional `sendto`/`recvfrom`/`sendmsg`/`recvmsg` are not a network relaxation because no other socket is obtainable. `archMap` without sub-architectures plus denied `personality` closes the AArch32 compat path.
- **Ingest→run handoff.** Full-stdin-write verification (`supervisor.rs:237`) closes the "tar exits early on a truncated archive" hole; the sole writer is force-removed *and* verified absent before Cargo starts; the run mount is read-only on both the requested and applied side and confirmed guest-side.
- **Detached-descendant cleanup.** `--init=false` makes cargo PID 1 of the namespace, so `rm --force` + verified absence does kill the `setsid()` grandchild via namespace teardown. The receipt's `top` output (two distinct SIDs) is a genuine observation.
- **Concurrency.** `calibrating`/`verified` are rechecked under the busy lock; the `Guard` leaves `verified == false` on any calibration failure; the observer thread's control commands are process-isolated from the job thread.
- **Volume ownership.** A pre-existing volume with foreign labels fails `Volume::parse` and is never deleted; `owned_volume` gates removal.

## Verdict

**Approve conditionally, for this bounded scope only** — calibration-gated local execution with no MCP exposure and no `project.inspect` completion claim. The containment design and its host- and guest-side verification are sound, and the supplied receipt is consistent with the code.

Two gates before this receipt is treated as authority:

- **P1-1 must be fixed and calibration re-run.** Until then, the `timeout` and `cancel` observations — and therefore `verified: true` and `configuration_fingerprint sha256:78d42c…` — do not carry the evidentiary weight the report asserts, because a masked harness error satisfies their pass criteria.
- **P2-1, P2-3 and P2-5 should be closed before the capability is wired to a public tool**, since they are what make a stored receipt durable and interpretable.

## Residual limits (accepted, not findings)

- The trusted daemon/host same-user mutation boundary is load-bearing throughout: volume identity is re-verified at creation but not re-inspected between ingest removal and run creation; `Volume::parse` accepts unknown fields; `mountpoint` is only prefix/suffix-checked.
- No hard quota on the managed volume against a compromised trusted extractor; bounded only by the finite generated archive. Documented in the ADR and correctly not advertised as a sandbox property.
- The `rust_applied` unit tests build on a recorded artifact with `Tmpfs["/work"]` and `Labels` mutated in-fixture (`rust_applied.rs:259-260, 281-283`), so verbatim echo of the current `/work` option string is proven only by the live gateway test, not by the unit test. The fixture comment states this honestly.
- Nothing verifies `/source` contents against the bundle digest inside the guest; integrity rests on tar's exit code plus the full-input-write check. Acceptable under the trusted-daemon assumption.
- The profile is calibrated against small fixtures. Omitted syscalls that a larger real crate might need (`mremap`, `membarrier`, `sched_setaffinity`, `openat2`) fail as `EPERM` from the default action and would surface as obscure build errors rather than identifiable containment events.
- Docker-version coupling is deliberate and fail-closed: the exact masked/readonly path lists and `Mounts.Mode == "z"` are pinned to 29.7.2 via `EngineIdentity`. Engine drift denies rather than degrades.
