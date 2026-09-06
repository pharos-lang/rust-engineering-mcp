# Independent review — ADR-061 (private persistent quality artifact store)

Reviewer: Claude Opus 5, effort high. Read-only. No commands run, no files changed.

---

## 1) Verdict

**Revise.**

The decision is architecturally sound in its posture (separate store, separate URI scheme, fail-closed recovery, no eviction, no daemon) and it correctly refuses the two dangerous shortcuts (mutating M1 semantics, interpreting guest archives/paths). But it contains one P1 that can damage M2 data availability, one P1 authority-granularity gap that its own oracle table would not catch, and a P1/P2 feasibility claim about APFS preallocation that the pinned dependency set cannot honour as written. Several P2s touch contracts and mandatory gates.

---

## 2) Answers to the seven questions (evidence first)

**Q1 — Does the `prepare_mutation_state` / `StateRoot` refactor risk M2 journal behaviour? Is the reuse plan realistic?**

Split answer.

- The `prepare_mutation_state` half is realistic and low risk. Its signature is `prepare_mutation_state(parent: &Path) -> Result<PathBuf, MutationError>` (`crates/project-adapter/src/mutation_state.rs:26`) and the only child-specific state is `const CHILD: &str = "rust-mcp-mutations-v1"` at line 53. Extracting `(parent, child_name)` is mechanical. Journal *bytes* cannot change: all journal encoding/decoding lives in `mutation.rs` (`encode`/`decode_envelope`/`canonical_checksum`, lines 692–843) and `prepare_mutation_state` never touches a journal.
- The `StateRoot` half is **not** realistic as written. `StateRoot` is a module-private struct with private methods (`crates/project-adapter/src/filesystem/macos/mutation.rs:254-397`). `write_new(&self, name, bytes: &[u8], phase: JournalPhase)` (line 364) takes a *mutation-journal phase* (used solely for test fault injection at line 389), buffers the entire payload in memory, and caps at `MAX_JOURNAL_BYTES` = 48 MiB. `read_optional` (line 340) has the same cap and returns `MutationError::RecoveryRequired` on stamp mismatch. None of these can carry a **streamed** 32 MiB blob with an exact byte cap, which the ADR requires at lines 186–189. Making them usable means changing M2 code, which contradicts "reuse … rather than copying it" + "leave the M2 callers … unchanged" (ADR-061:60-73). See **F4**.

**Q2 — Is "physical preallocation verified by the qualified native primitive" implementable on APFS? Recommendation?**

Half implementable, not verifiable. `rustix 1.1.4` (pinned, `Cargo.lock:4976`) *does* expose `fs::fallocate` on Apple, implemented as F_PREALLOCATE with `F_ALLOCATECONTIG`, falling back to `F_ALLOCATEALL`, then `ftruncate` (`rustix-1.1.4/src/backend/libc/fs/syscalls.rs:1787-1816`). But it passes `&store` by shared reference and **discards `fst_bytesalloc`**, so the caller cannot read back how many bytes were actually allocated; there is no `fcntl_preallocate` in rustix. Verifying via `st_blocks` is unreliable on APFS (delayed allocation, compression, clones, local snapshots). `crates/project-adapter/Cargo.toml:23-24` depends only on `rustix`; adding `libc` to touch `fstore_t` is a strategic dependency change requiring its own ADR (AGENTS.md:161-164, G7). Separately, the ADR's own "final blob is truncated to actual bytes" (line 169) *releases* the physical reservation before the descriptor commit — so the reservation is logical exactly in the window it exists to protect.

**Recommendation: replace the claim.** Define an *honest APFS reservation*: (a) pre-admission `fstatfs` free-space check against the sum of declared maxima **plus** a hard M2 headroom floor; (b) best-effort `fallocate` on each reservation file, documented as "not a hard APFS guarantee — snapshots, purgeable space and other writers can still consume space"; (c) **fail closed on ENOSPC/short write at every write and again at publication**, publishing no descriptor and releasing the reservation. Keep the "no logical accounting alone / no RSS / no Docker volume claim" rule; drop the word *verified*. See **F3**.

**Q3 — Owner binding consistency and cross-owner leak vectors.**

Inconsistent with what the code actually has, and it leaks across sessions.

- `ProjectRef` is a random `prj_` + 32 hex (`crates/domain/src/value.rs:95-99`), minted per `open` (`crates/application/src/lib.rs:118-136`), held in a **process-local** `HashMap` with a 1800 s default idle TTL (`crates/mcp-server/src/host_config.rs:18`). `ProjectIdentity` = `{workspace_root: String, fingerprint}` where the fingerprint hashes manifests only. The lease's physical identity is `Node{device: i32, inode: u64}` (`crates/project-adapter/src/filesystem/macos.rs:82-94`), private to the adapter.
- Two of the four binding inputs do not exist: there is **no host grant/policy identity** and **no session principal** anywhere in the registry. ADR-060 defines a `JobOwner` containing "an unforgeable server-side stdio-session principal"; ADR-061 omits it.
- **Leak:** binding = f(state secret, uid, root physical identity, grant). Two stdio servers — different peers, sequential or concurrent — with the same uid, the same `--state-root` and the same granted root derive the **same** binding. Peer B reads peer A's `SourceDerived` artifacts. The "Two owners" oracle (ADR-061:301) exercises two `ProjectRef`s, which under this binding are necessarily two different roots, so it passes while the real exposure is untested. See **F2**.
- Timing: the specified read order necessarily separates "no such artifact" (immediate ENOENT) from "another owner's artifact" (openat + descriptor parse + hash + compare). The blanket "no timing-oriented" claim (line 125) is unimplementable as stated. See **F10**.
- Index pagination: a job index "lists only members which the revalidated owner may read" — correct, but the page size and cursor length are unbounded numbers. See **F15**.

**Q4 — URI grammar collision and budget consistency.**

No collision, budgets fit but are unstated. `parse` in `crates/mcp-server/src/stdio/resources.rs:61-75` first requires `value.len() == 16 + 36 + 1 + 36 == 89` and then `strip_prefix("rust-artifact://")`; a `rust-quality-artifact://` URI is ≥ 97 bytes and fails both. Only `read_resource` is implemented (`crates/mcp-server/src/stdio.rs:213`) — there is no `list_resources`, so "resources/list does not enumerate" already holds. `prj_<32hex>` matches the real grammar. Budget: 320 KiB raw → 436,908 base64 bytes = ~83% of `MAX_RESPONSE` (512 KiB, resources.rs:28), leaving ~85 KiB of envelope headroom — it fits, but the ADR never shows this arithmetic and never bounds the *index* page, which has no proven fit.

**Q5 — Is clock-watermark fail-closed proportionate?**

Proportionate; it is precedent, not a new owner decision. `docs/security-model.md:151` already records for M1: "Una regresión del reloj limpia y bloquea el store." What is *new* and under-specified is the blast radius and the exit: "no artifacts until explicit operator recovery" with no defined operator action and no CLI (M2 has `mutation list/prune`, `docs/client-configuration.md:329-333`), and quota stays held indefinitely. See **F6**. Separately, making RFC3339 wall-clock authoritative for TTL contradicts the codebase rule "*Elapsed seconds from a process-local monotonic origin, never wall-clock UTC*" (`crates/application/src/lib.rs:51`) and ADR-060's "Deadlines use a monotonic clock … timestamps … never authorize work". See **F5**.

**Q6 — Does it block a JSON-only first vertical?**

Yes, by omission. M3-01's roadmap oracle is "nextest→job admitido→gateway→JUnit/log→Resource privado" (`docs/roadmap/m3-quality.md:25`). ADR-061 conditions the store on owner acceptance **and** native qualification but defines no interim path, while ADR-062 already assumes a "ships JSON-only initially" option exists (ADR-062:466-467). **It should allow staged adoption**, explicitly. See **F13**.

**Q7 — Anything beyond documentation that truly needs the owner?**

Yes, four things — the documentation edits (`SECURITY.md:99` "Artifacts M0 se almacenan solo en memoria (ADR-028)"; `docs/client-configuration.md:339`, which currently promises the state root holds only 128 journals/256 MiB with "No hay retención ni eliminación automática") are the *smallest* part:

1. **Disk-capacity coupling with M2** in the same state root — an operational/data-safety decision, not prose (**F1**).
2. **A new host permission class** for `SourceDerived`/`SymbolDerived` retention and export. That widens the host grant surface; AGENTS.md puts security first in the precedence list and reserves the security model to the Technical Owner.
3. **The persistence boundary**: whether retained evidence may cross *sessions and peers* on the same uid/root (**F2**). That is the actual privacy-posture change, and it is not what the ADR says it is.
4. **Uninstall/rollback data retention** — ADR-061 keeps the directories on uninstall; G6 forbids downgrading an anti-rollback floor to recover availability. Who is responsible for the leftover bytes is an owner call.

---

## 3) Findings

| ID | Sev | file:line | Claim | Why wrong / risky | Concrete fix |
|---|---|---|---|---|---|
| **F1** | **P1** | ADR-061:43-57, 160-172 | Sibling layout means "neither store scans or deletes the other", so the stores are isolated | Namespace isolation is not **capacity** isolation. Both children live under one `--state-root` on one APFS volume. The quality store physically preallocates up to 256 MiB globally; the M2 store may already hold 256 MiB (`mutation.rs:24`) and *assumes* it can always write its 48 MiB staging + 1 MiB metadata headroom (`mutation.rs:25-29`) — `ensure_new_record_quota` counts only its own directory bytes and never calls `statfs`. `docs/security-model.md:369-376` records that a journal truncated by a write failure "puede bloquear list/prune y commits nuevos de todo el store compartido". A quality preallocation can therefore drive the M2 writer into ENOSPC → `recovery_required` → whole mutation store blocked, needing manual operator remediation. | Before any reservation, `fstatfs` the state-root fd and require `free ≥ requested + M2 RECOVERY_HEADROOM (49 MiB) + gateway control headroom`; make that floor a named constant in the ADR. Or require the quality store to live on a distinct state root/volume. Add oracle: "a maximal quality reservation never pushes the mutation store below its recovery headroom; the M2 commit path still succeeds." |
| **F2** | **P1** | ADR-061:116-126, 301 | `owner_binding` isolates owners; owner B gets identical `Resource not found` for A's IDs | Binding inputs are state secret + uid + **root physical identity** + grant. All four are identical for two different stdio sessions (different peers, sequential or concurrent) run by the same uid against the same state root and the same granted root. B reads A's `SourceDerived`/`SymbolDerived` artifacts. The oracle only tests two `ProjectRef`s, which under this binding means two different roots — it passes while the actual novel exposure is untested. ADR-060 has the missing ingredient (`JobOwner`'s "unforgeable server-side stdio-session principal"); ADR-061 dropped it. Two of the four cited inputs (grant/policy identity) have **no representation in the code today**. | Either include a per-stdio-session principal in the binding (and accept that artifacts then die with the session, i.e. no cross-restart reuse), or state plainly that the boundary is **uid + state root + root grant**, record that in `SECURITY.md`, and add the discriminating oracle: *same root, two sessions* — currently absent. Also specify how "host grant/policy identity" is materialised, since it does not exist yet. |
| **F3** | **P1** | ADR-061:161-164, 169-170 | "physically preallocated and **verified** by the qualified native primitive" | rustix 1.1.4's Apple `fallocate` (`backend/libc/fs/syscalls.rs:1787-1816`) discards `fst_bytesalloc`, so allocation cannot be verified; `st_blocks` on APFS is not a faithful oracle (delayed alloc, compression, clones, snapshots); reading `fstore_t` needs a new `libc` dependency = strategic dep change (AGENTS.md:161-164, G7). And the ADR's own truncate-to-actual-bytes step releases the reservation *before* the descriptor commit, so the reservation is logical during the exact window it protects. | Adopt the honest-reservation formulation in Q2 above: pre-admission free-space check + best-effort `fallocate` + explicit "not a hard APFS guarantee" caveat + fail-closed on ENOSPC at every write and at publication. Move the truncate **after** descriptor publication, or state that surplus is released at truncate and adjust the accounting rule accordingly. |
| **F4** | **P2** | ADR-061:60-73; mutation.rs:254-397 | Reuse `StateRoot::{open,check,durable,read_optional,write_new}` rather than copying | All are module-private; `write_new` takes a `JournalPhase` and a fully-buffered `&[u8]` capped at 48 MiB; `read_optional` shares that cap and returns `MutationError::RecoveryRequired`. Neither can stream a 32 MiB blob with an exact cap. Generalising them **does** change M2 code, contradicting the same paragraph. The `prepare_mutation_state` extraction, by contrast, is genuinely safe (journal encoding is entirely in `mutation.rs`). | Scope the reuse to (a) the fixed-child helper extracted from `prepare_mutation_state`, and (b) a new `pub(crate)` handle-relative primitive module with a generic error type that both `StateRoot` and the quality adapter are refactored onto. Require "existing M2 mutation tests pass unchanged" as an explicit acceptance oracle. |
| **F5** | **P2** | ADR-061:107-108, 174-176 | RFC3339 `created_at_utc`/`expires_at_utc` are the authoritative TTL | Contradicts `crates/application/src/lib.rs:51` ("*never wall-clock UTC*") and ADR-060 ("timestamps exposed on the wire … never authorize work"). A wall-clock jump forward silently expires live artifacts; a jump backward trips the watermark and blocks everything. | Define the hybrid explicitly: durable wall-clock bound for cross-restart expiry only; monotonic deadlines in-session; wall-clock may only **shorten**, never lengthen, an in-session TTL; watermark detects regression. |
| **F6** | **P2** | ADR-061:174-176 | Clock regression → "serves no artifacts until explicit operator recovery" | Proportionate and precedented (`security-model.md:151`), but the ADR defines no operator action, no CLI, no authorization for it, and no bound on how long the block holds quota. M2 shipped `mutation list/prune` for exactly this reason. | Define the recovery command, its authorization, and what it may/may not delete; state explicitly that the block is scoped to quality artifacts and does not affect M1 Resources, M2 commit/receipt, or any of the 18 tools. Add that as an oracle. |
| **F7** | **P2** | ADR-061:243-249, 262-267 vs ADR-062:193-198, 513-515 | HTML/LCOV retention | Direct contradiction. ADR-062 says D17 must add `kind`/MIME/sensitivity to `ArtifactMetadata` (`crates/domain/src/artifact.rs:45-58`) and models HTML as a packaged **tar blob**, 8 MiB. ADR-061 leaves `ArtifactMetadata` untouched, defines a separate descriptor type, forbids archives, and models members as regular files at fixed guest paths, 32 MiB. Neither ADR says how a multi-file HTML report (directory of guest-named files) crosses the boundary at all. | Decide it here: (a) invoke the tool with a closed argv producing a single file at a fixed path; or (b) the **host** archives an enumerated report root with bounded depth/count/name-length and **host-generated** member names; or (c) HTML is out of scope for the first M3 verticals. Then correct ADR-062's dependency statement (it will not be satisfied by ADR-061 as drafted). |
| **F8** | **P2** | ADR-061:97-98, 222-223 vs ADR-060:49-51 | `qjob_<32hex>` / `qjr_<opaque>` | ADR-060 fixes `JobId` = `job_` + 32 hex for the same M3 job. Two ID vocabularies for one job invite a mapping that is itself an authority question, and make the D06-coordination sentence (ADR-061:139) unimplementable as stated. | Reuse ADR-060's `JobId`, or state the exact derivation and why the second identifier confers no authority. |
| **F9** | **P2** | ADR-061:134-139, 153 | 24 h max TTL; re-access after restart | Default `ProjectRef` idle TTL is 1800 s (`host_config.rs:18`) — *shorter* than the 1 h artifact TTL; ADR-060 bounds task records at 2 h. The ADR forbids `resources/list` and global search but never says whether the client may itself construct `rust-quality-artifact://prj_<fresh>/qart_<retained>`. If it may not, artifacts retained past a session are unreachable yet keep consuming owner/global quota to TTL — a self-inflicted admission denial. | State explicitly that a retained `qart_`/`qjob_` ID plus a freshly authorized `ProjectRef` **is** the re-access path (the ID is a locator, not a credential; binding revalidation is the gate) — or cap the maximum TTL at the session/task lifetime. |
| **F10** | **P2** | ADR-061:124-126 | "must not expose a different status, count, **timing-oriented scan**, descriptor, or existence signal across owners" | The specified order (resolve ref → derive binding → lookup) inherently costs more for "another owner's artifact" (openat + parse + hash + compare) than for "nonexistent" (ENOENT). The claim is unimplementable without constant-cost padding, which the ADR does not specify. | Narrow the claim to status/error-variant/count/enumeration indistinguishability, note that IDs are unguessable 128-bit so timing is not a practical oracle — or specify a fixed-cost read path and an oracle proving it. |
| **F11** | **P2** | ADR-061:44 (`store.lock`) | `store.lock` appears in the layout and nowhere else | No protocol, scope, or contention behaviour. `docs/security-model.md:333` records that "los locks solo coordinan servidores con el mismo state root" — so two concurrent servers can each believe they own the 256 MiB global budget, double-committing reservations. M2 solved this with `mutation-store.lock` + non-blocking exclusive `flock` (`mutation.rs:333-338`, 1178). | Specify: exclusive non-blocking lock held across admission/reservation/publication/reconciliation; contention returns a bounded "busy" rejection, never an unbounded wait; add a two-concurrent-process oracle. |
| **F12** | **P2** | ADR-061:79-82, 181 | The memory store "continues to implement only the M1 port" | Necessary but not sufficient. `ProjectRegistry::reap_artifacts` calls `store.retain_owners(&owners)` on **every** read path (`artifact_access.rs:50-58, 88-94`), deleting artifacts of any owner no longer in the in-memory registry — which would void persistence entirely if the durable store were ever wired there. Also `read_artifact` touches the lease on success (`:128-130`); the ADR says reads never renew *artifact* TTL but says nothing about the *project lease*. | State that the quality store is never passed to `reap_artifacts`/`retain_owners`, and decide explicitly whether an authorized quality read renews the `ProjectRef` idle TTL. |
| **F13** | **P2** | ADR-061:30-33, 289-306 | Store required for M3-01 evidence, but not authorized until owner acceptance + native qualification | No interim path is defined, so M3-01 has no artifact story in the meantime; ADR-062:466-467 already assumes one exists. | Add a staged-adoption section. **Stage 0** (default): quality jobs return bounded JUnit/log evidence through the **unchanged** M1 in-memory store (256 KiB/artifact, 1 h, process-local), setting `completeness: Truncated` and an explicit omission flag when a report exceeds it; no persistence, no new permission. **Stage 1**: the durable store, behind a named default-off host option, after owner acceptance and native qualification. |
| **F14** | **P3** | ADR-061:243-249 | "closed table of fixed guest paths (… declared report root)" | A *report root* is a directory of guest-named files, which is not a fixed path and is exactly the thing the same paragraph forbids interpreting. Coverage HTML and `cargo-mutants` output are both directories. | Either bound the enumeration explicitly (max depth, max entries, max name bytes, regular files only, no descent through symlinked dirs, host-generated member names) or remove the concept — resolve together with **F7**. |
| **F15** | **P3** | ADR-061:236-238, 222 | 320 KiB chunk / 512 KiB response | The arithmetic is correct (436,908 base64 bytes ≈ 83% of `MAX_RESPONSE`) but unstated, so a reader cannot check it; and the **index** page has no member-count or cursor-length bound, so its fit under 512 KiB is unproven. | State the base64 arithmetic as the justification for 320 KiB; fix a maximum members-per-page and a maximum cursor byte length. |
| **F16** | **P3** | ADR-061:100-113 | Descriptor fields `payload_format_version`, `source` ("… guest artifact name"), `runtime` | The ADR requires `deny_unknown_fields` and closed enums elsewhere, yet these three are prose, and "guest artifact name" puts guest-influenced text into a host descriptor returned to the client — re-introducing exactly what "no guest filename … becomes a host filename" excludes. | Make `source`/`runtime` closed typed structs; make the guest artifact name a **closed enum** over the fixed-path table, not a string. |
| **F17** | **P3** | ADR-061:289-306 | Oracle table is discriminating | Missing: ENOSPC / volume exhaustion; same-root two-sessions (**F2**); two concurrent processes (**F11**); "quality reservation does not break M2" (**F1**); M1 regression (URIs/limits/retention byte-for-byte unchanged); reservation released on mid-stream ENOSPC; `Linux/Windows → UnsupportedPlatform` *before* any guest output. "Crash mid-write" (line 304) tests a crash **after** blob + descriptor publication — the discriminating crash points are *between* blob rename and descriptor rename, and between descriptor rename and descriptor-directory fsync. | Add the seven missing cases; enumerate the intermediate crash points explicitly. |

---

## 4) Files read (for orchestrator hashing)

ADR / roadmap / policy:
- `docs/adr/ADR-061-private-quality-artifact-store.md` (full)
- `docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md` (full)
- `docs/adr/ADR-062-coverage-accounting-and-semver-baselines.md` (full)
- `AGENTS.md` (full)
- `docs/roadmap/m3-quality.md` (full)
- `docs/roadmap/m2-m8.md` (lines 61–195, G1–G8)
- `docs/roadmap/adr-backlog-m2-m8.md` (D06, D17, D18 sections)
- `docs/roadmap/traceability-m2-m8.md` (C11, L09–L13 rows)
- `docs/security-model.md` (lines 143–202, 320–384; plus keyword scan)
- `docs/client-configuration.md` (keyword scan: lines 225, 272, 325–350)
- `SECURITY.md` (keyword scan: lines 89–99, 136, 164–171, 239–245)

Code:
- `crates/project-adapter/src/mutation_state.rs` (full)
- `crates/project-adapter/src/filesystem/macos/mutation.rs` (lines 1–1577 of 2875; remainder not read — see Limitations)
- `crates/project-adapter/src/filesystem/macos.rs` (lines 72–150, 280–340 via grep context)
- `crates/project-adapter/Cargo.toml` (dependency section)
- `crates/mcp-server/src/stdio/resources.rs` (full)
- `crates/mcp-server/src/stdio/project.rs` (lines 1–120)
- `crates/mcp-server/src/stdio.rs` (lines 60–80, 205–275 via grep context)
- `crates/mcp-server/src/host_config.rs` (lines 14–22 via grep context)
- `crates/application/src/lib.rs` (lines 14–180 via grep context)
- `crates/application/src/artifact_access.rs` (full)
- `crates/domain/src/artifact.rs` (lines 13–115)
- `crates/domain/src/value.rs` (lines 93–101 via grep context)
- `crates/execution-adapter/src/state.rs` (lines 1–234)
- `Cargo.lock` (rustix entry, lines 4976–4979)

External (pinned dependency source, read-only):
- `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/rustix-1.1.4/src/backend/libc/fs/syscalls.rs` (lines 1786–1830)

---

## 5) Contradictions with normative sources and sibling drafts

1. **ADR-061 ↔ ADR-062 (artifact model).** ADR-062 §4 / Open issues requires D17 to add `kind`/MIME/sensitivity to `ArtifactMetadata` and models HTML as a packaged tar blob (8 MiB); ADR-061 leaves `ArtifactMetadata` alone, uses a separate descriptor, forbids archives, and caps at 32 MiB. Neither defines the multi-file HTML crossing. → **F7**.
2. **ADR-061 ↔ ADR-060 (job identity).** `qjob_<32hex>` vs `JobId` = `job_` + 32 hex. → **F8**.
3. **ADR-061 ↔ ADR-060 + `crates/application/src/lib.rs:51` (clock).** Wall-clock RFC3339 as authoritative expiry vs "monotonic clock; timestamps never authorize work". → **F5**.
4. **ADR-061 ↔ `docs/security-model.md:369-376` + `docs/client-configuration.md:339` (state-root capacity).** The M2 store's documented 207 MiB admission retention + 48 MiB staging + 1 MiB growth assumes the volume can absorb it; ADR-061 adds 256 MiB of preallocation to the same volume without reconciling. → **F1**.
5. **ADR-061 ↔ ADR-060 (authority vocabulary).** ADR-060's `JobOwner` binds a stdio-session principal; ADR-061's `owner_binding` deliberately excludes any session identity, producing two different notions of "owner" for the same M3 job. → **F2**.
6. **ADR-061 ↔ G3 (`m2-m8.md:105-106`).** "Cada nuevo límite se fija antes del código con **unidad, fase cubierta, valor por defecto/máximo y prueba del exceso**." The budget table (ADR-061:146-155) gives a single value per row with no unit/phase column and only the TTL row has a default/maximum split; not every row has a corresponding exceed-oracle.
7. **ADR-061 ↔ L11 (`traceability-m2-m8.md:61`).** L11 lists *eviction* among the M3-01 evidence; ADR-061 rejects eviction outright. That is a defensible decision, but it is a divergence from a normative traceability row and should be recorded as such rather than passed over in Alternatives.
8. **AGENTS.md:50** — "No avanzar a M3 … sin autorización explícita adicional." Consistent with `Status: Proposed`, but the Decision section is written in adopted imperative voice ("Add a distinct, host-only child…"); worth a one-line scope guard so it cannot be read as authorization.

---

## 6) Missing decisions the ADR should make before implementation

1. **Free-space policy and the M2 floor** — the constant, the syscall, and the failure code (**F1**).
2. **What "equivalently authorized ProjectRef" means precisely** — same root inode? same manifest fingerprint? same session? Today a manifest edit changes `ProjectIdentity.fingerprint` and invalidates the ref, while the root inode is unchanged; the ADR's binding would still match (**F2**, **F9**).
3. **Whether a client may construct a URI from a retained `qart_` ID and a fresh `ProjectRef`** (**F9**).
4. **`store.lock` semantics** — scope, blocking behaviour, and what it covers (**F11**).
5. **The multi-file report crossing** — argv-fixed single file, host-side bounded archive, or out of scope (**F7**, **F14**).
6. **Whether an authorized quality read renews the `ProjectRef` idle lease** (**F12**).
7. **The name and default of the host option that enables persistence**, and the staged-adoption ladder (**F13**).
8. **Operator recovery procedure** for quarantine and clock-watermark states — command, authorization, what it may delete, quota release (**F6**).
9. **Index page size and cursor grammar** as numbers (**F15**).
10. **Where the state secret comes from and its rotation/loss semantics** — what happens to retained artifacts if the secret file is missing or unreadable (currently unspecified; presumably all bindings fail, which is a silent total loss of accessibility while bytes keep consuming quota).
11. **`payload_format_version` closed values per kind**, and the closed `source`/`runtime` types (**F16**).
12. **Whether `SecretSuspected` is a retention refusal or a permission escalation** — line 277 says it "trigger[s] the stricter permission/withholding policy" without saying which.

---

## 7) Limitations of this review

- Read-only, as instructed: no commands, no builds, no tests, no hashing. All byte counts and arithmetic are computed by hand from the sources listed.
- I read `crates/project-adapter/src/filesystem/macos/mutation.rs` lines 1–1577 of 2875. The unread tail contains `scan_store`, `recover_locked`, `swap`/`verify_swap`/`cleanup_temp` and the test module. My M2 findings rest on the constants (lines 22–38), `StateRoot` (254–397) and `ensure_new_record_quota` (1136–1157), all of which I read directly; but I have **not** verified whether the unread tail performs any state-root-wide directory scan that could observe a sibling child (it operates on `self.state.directory`, which is the mutations child, so a sibling is out of reach — but I did not read the code to confirm).
- APFS behaviour of `F_PREALLOCATE` under snapshot/purgeable-space pressure is from documented semantics plus the rustix source, not from measurement on this host. The recommendation in Q2 is deliberately framed so that it holds either way.
- I did not read the full spec (`docs/spec/rust-engineering-mcp-propuesta-v0.3.md`); §77/§26 claims are taken from the roadmap's and ADR-062's quotations of them.
- ADR-062 was reviewed only where it touches ADR-061 (artifact model, retention, D17 dependency). It has its own review package.
- The A17 delegation transcript under `docs/validation/m3-delegation/` was treated as content under review, not as evidence.

---

## 8) Proposed disposition

| ID | Disposition |
|---|---|
| F1 | **Fix now.** Blocks acceptance — it can damage M2 data availability, and M2 is closed/qualified work. |
| F2 | **Fix now.** Either add the session principal or restate the boundary honestly in the ADR + `SECURITY.md` + the oracle table. This is the owner-decision item. |
| F3 | **Fix now.** Rewrite the reservation paragraph; it is the ADR's load-bearing feasibility claim and it cannot be implemented as worded with the pinned deps. |
| F4 | **Fix now** (wording + scope). Cheap: narrow the reuse claim and add the M2-regression acceptance oracle. |
| F5 | **Fix now.** One paragraph; prevents an implementation from diverging from ADR-060 and the existing clock rule. |
| F6 | **Fix now** for the scoping sentence + recovery-procedure stub; the CLI itself may **defer** to M3-01 implementation. |
| F7 | **Fix now.** Two ADRs currently disagree; leaving it makes both unimplementable for HTML/LCOV. Choosing (c) "out of scope for now" is an acceptable resolution. |
| F8 | **Fix now.** Trivial and prevents a second authority vocabulary. |
| F9 | **Fix now** for the re-access rule; **defer** the TTL-maximum recalibration to fixture measurement, as the ADR already marks budgets provisional. |
| F10 | **Fix now** (wording). An over-claimed security property is worse than a stated limitation. |
| F11 | **Fix now.** ADR-050's local-coordinated model makes multi-process sharing the expected case. |
| F12 | **Fix now** (two sentences). |
| F13 | **Fix now.** Without it M3-01 has no artifact path and the ADR becomes a hard blocker for the first vertical. |
| F14 | **Fix now** if F7 keeps report roots; otherwise resolved by F7. |
| F15 | **Fix now** for the index page/cursor bounds; the 320 KiB arithmetic note is **defer** (nice-to-have). |
| F16 | **Fix now** for the guest artifact name (it is a containment property); **defer** the closed `source`/`runtime` shapes to M3-01 domain design, recorded as an Open issue. |
| F17 | **Fix now.** The oracle list is the ADR's own acceptance gate; as it stands it would certify a store with F1, F2 and F11 present. |

**Nothing is rejected.** The core structure — separate versioned store, separate URI scheme, descriptor-as-commit-marker, no eviction, no daemon, fail-closed recovery, no archive/path interpretation — should be kept. Add an **Open issues** section (ADR-062 has one; ADR-061 does not), which is where several of the deferred items above belong.