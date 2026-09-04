# M1 source capture — independent review and disposition

Claude Code2.1.259, explicit claude-opus-5 High, read-only packet, tools disabled,
safe/restricted mode, no session persistence. Actual modelUsage confirms Opus5;
auxiliary Haiku telemetry also present. No execution or edits by external reviewer.

## Principal disposition

- P1-1 fixed: Cargo/toolchain TOML preserves256KiB parser cap. Exact/+1 and512-deep
  arrays/inline tables in standalone manifests discriminate the pinned parser's
  real max-recursion-depth rejection. No parallel handwritten TOML parser added.
- P1-2 conditional gap not confirmed: principal read manifest.rs. validate starts
  with validator.read(root), follows explicit members/dependency/patch edges, and
  rejects any missing manifest/target rather than returning a partial graph.
  package.workspace must resolve exactly to selected root. External/excluded edges
  reject capture; no broader authority is granted. Ancestor workspace discovery is
  an existing explicit ADR-024 limitation, not redesigned by this cut.
- P2-1/3/6/7/12 fixed where applicable: case-insensitive exclusions/config rejection
  (including config directories), inner disappearance maps to InvalidProject,
  relative path checked before I/O, excluded Cargo paths rejected, exclusive fixture
  root creation. Registry still reports ProjectNotFound for a genuinely dead lease.
- P2-2 already addressed by ADR-024: O_UNIQUE is defense in depth; fstat independently
  rejects nlink!=1. Source tests exercise static and post-enumeration hardlinks.
  No new dependency on an unprobed flag and no M0 redesign.
- P2-4 rejected: USTAR typeflag5 does not require a trailing slash. Domain retains
 100-byte directory names. The separate real Rust gateway/ingester experiment
  successfully materialized such a directory and a build.rs asserted its existence;
  gateway calibration itself remains unfinished.
- P2-5/8/9/10: explicit subset/observational limits. Host absolute-path budgets also
  apply. No atomicity/inode-history claim for descendants. Multiple bounded manifest
  walks preserve backend and registry guarantees; the shared worker/deadline guards
  apply when wired to MCP. Literal path patterns only, no glob expansion. ADR-031
  and validation document these constraints; no unbounded-rate throughput claim.
- P2-11 suggestion rejected: a mutation before the file's first observation can
  legitimately be captured, as can one after final observation. Requiring rejection
  at every earlier callback would claim atomicity the design does not provide.
  Dedicated post-read mutation, stale d_type FIFO/hardlink and external-sentinel
  races provide discriminating containment coverage. Sweep does not claim stronger
  snapshot semantics.
- Five further tests and complete core gate pass after these changes. No confirmed
  blocking source-capture finding remains. This review does not certify execution.

## Independent output (not authoritative)

## Scope and method

Read-only review of the eight files supplied. No tools, no execution, no repository reads. I did **not** see `crates/project-adapter/src/manifest.rs` (the M0 `manifest::validate` graph walker), `crates/domain/src/lib.rs`, or the untracked `crates/execution-adapter/{rust_applied.rs,seccomp-rust.json,state.rs}` from the branch status — findings that depend on those are marked conditional.

Nothing here certifies sandboxing. This patch implements host-side capture only: no tar encoder, no volume/ingestion orchestration, no Cargo invocation, no seccomp profile. ADR §75–81 (flock / `AF_UNIX` `SOCK_SEQPACKET` deltas) is unverifiable from the supplied set.

---

## P0

**None found.** The capture path's core containment argument holds under the ADR's stated assumptions:

- `d_type` is used only to select open flags; the actual kind is re-validated on the no-follow descriptor (`macos/source.rs:75`, `88`, `123` → `macos.rs:103`). Substitution races are covered by `entry_race_tests` (`macos/source.rs:516`).
- `fstat` precedes any `read` (`macos/source.rs:123` before `131`), so FIFOs/sockets/directories/devices are rejected without content access.
- `st_nlink != 1` rejects hardlinks at capture *and* at `recheck()`, since `FileStamp::from_stat` fails rather than compares (`macos.rs:103`, `macos/source.rs:159`).
- The `NOFOLLOW_ANY`/`RESOLVE_BENEATH` startup probes fail **closed**: if the kernel silently ignores either bit, `openat(&slash, "/", …)` succeeds and `SecureProjects::new` returns `unsupported()` (`macos.rs:181–189`). This is the right shape for an unverifiable-flag problem.
- The device-equality check (`macos.rs:246–248`) catches firmlinks, mounts and cross-volume APFS constructs that `O_NOFOLLOW_ANY` does not.
- No bytes escape a post-capture identity change: `application/src/source.rs:32` propagates the revalidation error and drops `result`; `macos/source.rs:415` binds the bundle to the same fingerprint observed at `380`.
- Manifest bytes are *strongly* bound (the fingerprint hashes manifest contents, compared before and after capture), so the shipped manifests provably equal the attested ones. Non-manifest files are bound only by the stamp `recheck()` — which is precisely the non-atomic guarantee ADR:26 claims, not a bug.

---

## P1

### P1-1 — Manifest TOML parse budget regresses 4× against M0, on a wider input set
`crates/project-adapter/src/filesystem/macos/source.rs:149`, `182–183`, `198–199`

`Capture::file` reads up to `SOURCE_MAX_FILE_BYTES` (1 MiB, `domain/src/source.rs:7`) and then hands the bytes to `toml::from_str` in the **host gateway process**. The M0 reader caps the identical parse at `MAX_FILE_BYTES` = 256 KiB (`macos.rs:25`, `379–390`). So the same `Cargo.toml`, read via a different code path, gets a 4× larger parser budget — and `validate_configuration` now parses *every* `Cargo.toml` in the subtree, including standalone manifests the M0 graph never visits (deliberately, per the comment at `macos/source.rs:206–207`). Aggregate exposure is bounded only by `SOURCE_MAX_TOTAL_BYTES` (16 MiB), i.e. ~16 adversarial 1 MiB manifests per capture.

Recursive-descent TOML parsers have a history of stack exhaustion on deeply nested inline tables/arrays; a stack overflow aborts the process, and this input is untrusted by construction. I cannot confirm the pinned `toml`/`toml_edit` version's depth guard from the supplied files.

Fix: cap manifest bytes at the M0 `MAX_FILE_BYTES` before `toml::from_str`, and add an explicit nesting-depth pre-scan (or assert the parser's documented depth limit in a test). The size asymmetry between the two readers is worth removing regardless of the parser question.

### P1-2 — `CapturedIo` silently changes `ManifestIo` semantics; the fingerprinted graph and the validated graph may differ
`crates/project-adapter/src/filesystem/macos/source.rs:352–373`

`manifest::validate` is run twice with two different IO implementations over the same logical project:

| | `Access` (`macos.rs:374–405`) | `CapturedIo` (`source.rs:356–373`) |
|---|---|---|
| path outside the selected subtree, inside a configured root | live read succeeds | `strip_prefix` fails → `denied()` (hard error) |
| path under `.git` / `target` | live read succeeds | `Ok(None)` / `false` (absent) |
| file size cap | 256 KiB | bundle contents (1 MiB) |

Two consequences:

1. **Attestation gap.** The identity fingerprint is computed by `collect()` over the *live* filesystem graph (`macos.rs:267`), while the bundle is validated over `CapturedIo`. If the validator ever resolves an edge outside the subtree or into an excluded directory, the graph you fingerprinted is not the graph you validated. The bundle would still be safe (only captured bytes ship), but the security argument "the registered manifest identity attests the transferred manifests" no longer closes.
2. **Error-code confusion on a common layout.** If `manifest::validate` ascends to find a workspace root — the ordinary case of opening `/repo/crates/foo` inside a `/repo` workspace — `open()` succeeds and `source()` fails with `SandboxDenied` from `strip_prefix` at `source.rs:359`. ADR:95 permits *rejecting* such projects; it does not license reporting a subset limitation as a sandbox denial. A sandbox-denied code will be read as a containment event in triage.

Conditional on `manifest.rs`, which I could not read. Fix either way: make the out-of-subtree case a typed subset rejection at `source.rs:359`, and add a test that a member crate of an ancestor workspace produces that typed code rather than `SandboxDenied`. If the validator can ascend, either restrict `collect()`'s graph to the subtree for source-eligible projects, or fingerprint over the captured bytes.

---

## P2

**P2-1 — `.git`/`target` exclusion is case-sensitive on a case-insensitive filesystem.**
`macos/source.rs:89`. macOS APFS is case-insensitive and case-preserving by default, so a directory created as `Target` or `.Git` is returned by `readdir` under that spelling and is *not* excluded; its contents are captured and transferred. `.Git/config` can carry credentials. Real-world reachability is low (git and cargo create these lowercase, and both names cannot coexist on one volume), but the ADR:34 claim "Exclude .git and target directories" is not met on the actual target filesystem. Use `eq_ignore_ascii_case`. Note this does **not** affect the `.cargo` guard at `source.rs:111–121`: a case-variant `.Cargo/Config.toml` is inert on the case-sensitive Linux guest.

**P2-2 — `UNIQUE` (0x2000) is applied but never probed.**
`macos.rs:23`, used at `macos.rs:60` for non-directory opens only. `NOFOLLOW_ANY` and `RESOLVE_BENEATH` each have a startup probe that fails closed; `UNIQUE` has none. `open(2)` silently ignores unknown flag bits, so the code cannot distinguish "enforced" from "no-op", which contradicts the file's own stated standard at `macos.rs:161–162` ("Headers and OS branding alone do not demonstrate enforcement"). Probe it or delete it.

**P2-3 — An inner-file deletion reports `ProjectNotFound`, which means "your lease is dead".**
`macos.rs:41–42` maps `ENOENT` → `ProjectNotFound`; `recheck()` (`source.rs:159`, `167`) and the mid-capture opens propagate it verbatim, and `application/src/source.rs:33` returns it unchanged while the entry remains live. A client that deletes a file mid-capture is told its project handle is gone and will re-open unnecessarily. Distinguish "lease target missing" from "captured entry vanished" — the latter is a race rejection (`InvalidProject`).

**P2-4 — A 100-byte directory path cannot be encoded in USTAR.**
`domain/src/source.rs:6`, `28`, and `89`. `SOURCE_MAX_PATH_BYTES = 100` matches the USTAR `name` field exactly, but directory members conventionally carry a trailing `/`, making a 100-byte directory 101 bytes. `with_directories(vec![], vec!["a".repeat(100)])` is accepted today; the test at `domain/src/source.rs:235–238` only pins the 101-byte case. The encoder is not in this patch, so this is latent — bound directory paths to 99 bytes at `source.rs:89`, or reserve the separator in `validate_source_path` when validating a directory.

**P2-5 — `checked_path`'s absolute-path caps silently shrink the advertised relative budget.**
`macos.rs:71` rejects any absolute path with >64 components (and `:69` >4096 bytes). Effective source depth is `64 − root_depth`, not the documented 32 (ADR:39), and the failure is `InvalidProject` mid-capture rather than the documented typed limit rejection (`OutputLimitExceeded`). A project registered at depth 40 fails at depth 24 with a wrong-looking code. Either derive the check from the root depth plus `SOURCE_MAX_DEPTH`, or map the overflow to the limit code.

**P2-6 — `Capture::file` reads before computing and validating the relative path.**
`macos/source.rs:122` opens and `131–133` reads; `strip_prefix(self.base)` and `validate_source_path` only happen at `144–149`. Not currently exploitable — every production caller reaches `file()` from `directory()`, which validated the relative path at `72` — but the containment invariant is enforced by the caller rather than the function. Hoist the `strip_prefix`/validate to the top of `file()`; it costs nothing and makes the direct-call path (used by the test at `source.rs:474`) safe by construction.

**P2-7 — `cargo_path` accepts manifest paths that resolve into excluded directories.**
`macos/source.rs:325–349`. `[lib] path = 'target/generated.rs'` or `build = '.git/x.rs'` normalizes to a valid in-subtree source path and passes, but the referenced file is never captured. Accepted at capture, opaque failure at build time inside the container. Reject resolved paths whose first component is `target` or `.git` at `source.rs:341`.

**P2-8 — Descendant descriptors are dropped before `recheck()`, so identity rests on APFS not reusing inode numbers.**
`macos/source.rs:156–172`. `child` and `fd` are released at the end of each `directory()` frame; `recheck()` re-resolves by path and compares `(dev, ino, mtime, ctime)` with nothing pinning the original inodes. `macos.rs:143` shows the authors treat pinning as load-bearing for the lease root ("preventing inode reuse on revalidation") — the descendants get no equivalent. APFS's monotonic 64-bit file IDs and the `require_apfs` enforcement make this sound in practice; it should be stated as an explicit assumption alongside ADR:29, or the descriptors retained (bounded at 4096, which will collide with `RLIMIT_NOFILE` — so documenting is likely the right call).

**P2-9 — Cost amplification per `source` call.**
`application/src/source.rs:22`, `32` plus `macos/source.rs:380`, `415` produce **four** full manifest-graph walks per capture, each re-reading and re-stamping every manifest, on top of the traversal itself. `open_path` additionally issues two `check_root` rounds and an `fstatfs` per entry (`macos.rs:234`, `245`, `249`), i.e. ~5 extra syscalls × 4096 entries. There is no per-reference rate limit on `source`. Bounded per call, but a client can loop. Worth a bound at the MCP admission layer (commit `dc53c72`) and a note that the double revalidation at `source.rs:415` is redundant with the registry's own post-capture `resolve_inner`.

**P2-10 — Glob rejection is an undocumented consequence.**
`macos/source.rs:287–300` and `228–234` route `include`/`exclude`/`members`/`default-members` through `cargo_path`, whose raw-component check rejects `*`, `?`, `[`. `members = ["crates/*"]` is the dominant workspace idiom and is rejected. The code comment at `285–286` is honest about it; ADR:31–40 is not, and ADR:95 covers it only generically. Add one sentence to the ADR's initial-subset paragraph — this will be the most common rejection in practice.

**P2-11 — The race sweep asserts too weakly to catch a regression.**
`crates/project-adapter/tests/source.rs:304–336` iterates every checkpoint but accepts `Ok` at any of them and only asserts `rejected_count > 0`. An implementation that detects the mutation at exactly one checkpoint passes. The comment at `327–328` correctly identifies that only mutations *after the final observation* may be accepted — assert that: rejection required for all `at` strictly below the last checkpoint, `Ok` permitted only at the final one.

**P2-12 — Minor.** `.cargo/config.toml` created as a *directory* bypasses the file-name guard at `source.rs:111–121` and its contents are captured (inert on Linux — Cargo gets `EISDIR` — so cosmetic). `capture.files` is `mem::take`n at `source.rs:405` while `capture` stays live through `recheck()` at `418`; correct today, fragile if anyone adds a `files`-based check. Test fixtures build under world-writable `/private/tmp` (`source.rs:31`, `source.rs:432`) and rely on `OsReferences` entropy plus `create_dir` failing on collision — `Fixture::new` uses `create_dir_all` at `tests/source.rs:33`, which does not fail on a pre-existing base.

---

## Documented assumptions and subset limits — not bugs

Explicitly excluded from the findings above, as covered by the attached ADR:

- **Non-atomic capture** (ADR:26, 28). The `Ok` branch at `tests/source.rs:327` and the swap-and-restore window between the two `open_path` calls at `source.rs:87` and `93` are the stated guarantee, not defects. Detection is stamp-based and fails closed; atomicity is never claimed.
- **Privileged-device / mount / trusted-root assumptions** (ADR:29). Device nodes inside the subtree require root to create; `ctime` forgery requires `settimeofday` or equivalent. The whole `FileStamp`/`DirectoryStamp` argument rests on an unprivileged attacker being unable to alter content without moving `ctime` — correctly declared, correctly out of scope.
- **Restricted transfer subset** (ADR:31–40, 95): rejection of symlinks and all non-regular objects, project `.cargo` config, external/absolute Cargo paths, non-`1.98.1` toolchain files, and the loss of file permissions (`build.rs`-invoked shell scripts lose `+x` under the planned 0444). All fail closed; the "can reject projects accepted by structural `project.open`" consequence is stated.
- **No local-volume quota** (ADR:59–60, 91) and **ingestion is not project-code execution** (ADR:61) — out of this patch's scope entirely.
- **Build scripts and proc macros are admitted into the bundle by design.** ADR:71–73 gates their *execution* behind independent adversarial calibration of the exact runtime/profile/transfer path. Nothing in this patch runs them, and nothing here should be read as progress toward that gate.

## Bottom line

The capture path is well-constructed and the containment reasoning is sound under its declared assumptions; I found no P0. The two P1s are both about the *seam* between M0's live-filesystem readers and this patch's captured-bytes readers — a parser budget that got 4× looser crossing that seam, and a `ManifestIo` substitution that changes verdicts for out-of-subtree and excluded paths. Both are fixable without touching the containment design. The P2 list is mostly hardening, error-code hygiene, and one ADR-accuracy item (glob rejection). Recommend closing P1-2 by reading `manifest.rs` before merge, since its impact is conditional on whether the validator ascends past the selected subtree.
