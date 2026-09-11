## Read-only security review — M1-10 authenticated import + APFS durable store

Scope reviewed: `docs/adr/ADR-041`, `crates/catalog-adapter/src/bundle.rs`, `bundle/archive.rs`, `bundle/tests.rs`, `crates/project-adapter/src/catalog_store.rs`, `tests/catalog_store.rs`, `crates/mcp-server/src/catalog_cli.rs`, `tests/catalog_cli.rs`. No tools, commands, or edits were run; test results (bundle 7 / store 12 / CLI 3) are taken as reported by the principal, not independently executed. Native semantic persistence, network sync, and rebuild wiring are treated as incomplete.

**Verdict: no P0. Two P1s. The core cryptographic and archive-parsing design is sound** — the property that matters most (nothing untrusted reaches SQLite/JSON parsers or the filesystem-name layer) holds.

---

## What holds up

**Verification-before-parsing is real.** The only pre-authentication attack surface is zstd (`bundle.rs:145-149`, window capped at 2^23, output capped at 80 MiB via `bundle.rs:156-169`) and the tar header parser (`archive.rs`, pure safe Rust, every offset bounds-checked, `checked_add` on `end`/`padded` at `archive.rs:66-70`). Signature verification at `bundle.rs:185-187` precedes `serde_json::from_slice` at `:188` and `SqliteCatalogRepository::open` at `:236`. The 16 KiB manifest bound at `:176` is applied before the verify call, so the signed message is ≤16 KiB + 35.

**Payload set is fully determined by the signed manifest.** `entries[0]`/`entries[1]` are name-pinned (`:172`), the count is exact (`:208`), and every remaining entry is matched by name, length, and SHA-256 against a strictly-ascending manifest list (`:212-224`). There is no unlisted-member, duplicate, or reordering slack. `files[0].path == "catalog.sqlite"` is genuinely implied by the ascending check plus `:225`, so the `manifest.files[0].sha256` indexing at `:232` is safe.

**Archive names never become paths.** Confirmed by inspection: nothing in `bundle.rs` or `archive.rs` writes using an entry name; the store writes only `active.bundle` / `staging.bundle` / `store.lock`. Traversal, absolute names, prefix-field splitting (`archive.rs:25`), linkname (`:24`), non-regular typeflags (`:23`), device numbers (`:26-27`), and GNU base-256 numerics (`:46`, rejected by the UTF-8/0-7 check in `octal`) are all rejected. Path charset and component rules at `:50-63` are strict, and the `matches!` allowlist at `bundle.rs:220` reduces the reachable set to three fixed names.

**Canonical manifest enforcement is correct and self-tightening.** The round-trip byte comparison at `bundle.rs:190-192` subsumes `deny_unknown_fields` for nested types (an unknown field inside `Provenance` would be dropped on re-serialize and mismatch) and rejects omitted `Option` fields, escaped strings, and whitespace variants — all fail closed.

**Domain separation is adequate.** `SIGNING_CONTEXT` (`bundle.rs:14`) is a constant-length prefix ending in `\0`; with exactly one message type signed by this key, it is unambiguous. No TOFU: the key comes only from the host trust file, and `publisher`/`channel` are bound inside the signed manifest and cross-checked against trust (`:195-197`), so a same-key cross-channel replay fails.

**Commit atomicity is correctly ordered.** File `fsync` + `fcntl_fullfsync` before `renameat` (`catalog_store.rs:396-397, 410`), directory `fsync` + `fullfsync` after (`:282-285`), staging inode re-stamped and compared immediately before rename (`:399-408`), and every post-rename failure collapses to `DurabilityUncertain` with no undo (`:411-426`). `commit` refuses to treat a hostile `active.bundle` as absent (`:379-384`). The CLI adds a full byte readback (`catalog_cli.rs:175-177`).

**Handle discipline is unusually thorough.** `NOFOLLOW_ANY` is *behaviorally probed* before it is relied on (`catalog_store.rs:164-172`), `st_nlink == 1` rejects hardlink swaps (`:115`), `st_dev` is pinned to the root (`:233`), APFS is verified per-descriptor (`:79-91`), and the lock-file inode is re-checked after `flock` to catch the unlink-and-recreate race (`:332-341`).

---

## P1

### P1-1 — The trust anchor is read with no ownership or mode validation

`catalog_cli.rs:156` loads the root of trust via `read_catalog_file`, which calls `Root::open(parent, /*is_private=*/ false)` (`catalog_store.rs:437`) and `root.read(..., self.private == false)`, so `private()` (`catalog_store.rs:92-96`) is **never** applied to the trust file or its parent directory. The store itself enforces owner + `0o700`/`0o600` (`:183-185`, `:121-123`); the trust file — the only thing that decides which publisher can activate a catalog — gets none of that.

What *is* enforced: APFS, regular file, `nlink == 1`, no symlink in any path component, `st_dev` match. None of those stop a `rename()` over the path by anyone with write permission on the containing directory.

**Exploit assumption (stated plainly):** the operator points `--trust` at a path in a directory writable by a principal other than the store owner — `/tmp`, a group-writable `/usr/local/etc`-style config dir, or a shared admin drop location. The attacker atomically renames their own `{publisher, channel, public_key}` JSON over it and leaves a bundle they signed where the operator's import runbook expects one. The next operator-run `import` reports `status: passed` and durably activates an attacker-authored catalog: crate metadata, versions, licenses, and RustSec advisories. Secondary effect: the previously active bundle no longer verifies under the substituted key, so the store also becomes unrecoverable except by deletion (see P1-2).

This is not exploitable if the trust file sits beside the store in a `0700` owner-only directory — but nothing in the code, the CLI, or ADR-041 requires or checks that, and the whole point of the `is_private` machinery elsewhere is to not depend on operator path hygiene.

**Fix:** pass `is_private = true` for the trust read, and reject group/other-writable ancestors of the trust path (the ancestor walk is cheap given the ≤64-component bound already enforced at `catalog_store.rs:65`). At minimum, document the requirement in ADR-041 and fail closed on a non-owner-owned trust file.

### P1-2 — No sequence floor independent of the active record; a corrupt active blocks forward imports, and the only recovery resets the floor to 0

The floor lives exclusively inside `active.bundle` (ADR-041:35-36). `catalog_cli.rs:160-163` verifies the active record *before* dispatch, and `?` aborts the entire command on failure. Consequently a corrupt or truncated active record blocks not just rollback but **legitimate forward imports** — you cannot import sequence *N+1* to repair a damaged sequence *N*.

The only available recovery is deleting `active.bundle`, after which `active.map_or(0, ...)` at `catalog_cli.rs:171` sets the floor to 0 and *any* older validly-signed bundle imports cleanly. The packet ships no recovery command, no `--reset`, and no documented procedure; ADR-041:66 explicitly declines a downgrade flag.

I am aware the review brief excludes "same-user deliberate store deletion/restore." I am raising this anyway because the trigger here is not a deliberate rollback: a single flipped bit, a truncated write from an unrelated tool, or an out-of-space condition during any external copy forces the operator into a floor reset as the *only* path back to a working store. The ADR's claim that "recovery never falls back to an older bundle" (line 39) is true of the code path but false of the operational reality it creates.

`crates/mcp-server/tests/catalog_cli.rs:38-39` tests exactly this state and asserts the import is refused — correct behavior, but it documents the dead end rather than resolving it.

**Fix:** persist a small monotonic floor record (`floor.seq`) durably *before* the rename, never decreased, validated independently of the active bundle. On a corrupt active, allow forward import subject to the persisted floor. This also fixes the key-rotation gap in P2-6.

---

## P2

**P2-1 — Peak import memory is a multiple of the 80 MiB budget.** `catalog_cli.rs:160-171` keeps the fully-materialized `active` `VerifiedBundle` (input bytes + decompressed archive + owned SQLite copy + rustsec/index copies) alive across candidate verification and `commit`, though only `.manifest().sequence` is needed. Two 80 MiB generations decoded concurrently plausibly peaks in the high hundreds of MiB. ADR-041:97-98 disclaims an RSS bound, but the fix is trivial: extract the sequence and drop `active` before `read_catalog_file` at `:169`.

**P2-2 — `bundle_sha256` is malleable for a fixed signed manifest.** The signature covers only the manifest; the container is unsigned. Tar header slack that `archive.rs` validates-but-does-not-constrain (mode/uid/gid `100..124`, mtime `136..148`, uname/gname `265..329` — the latter two are not checked at all) plus zstd level/framing and extra trailing zero blocks all change `sha256(bytes)` at `bundle.rs:258` while verifying identically. Any operator or fleet tool comparing `bundle_sha256` across hosts to detect tampering will see spurious mismatches, and an `active.bundle` can be re-encoded in place undetected. Report `catalog_sha256` + `sequence` as the generation identity, or state in the report schema that `bundle_sha256` is container-level only.

**P2-3 — `semantic_index_available` is meaningless as reported.** Hardcoded `false` at `catalog_cli.rs:136` even when the verified bundle carries a `semantic.index` member; `VerifiedBundle::semantic_index_bytes()` (`bundle.rs:124`) is dead code in this packet. `catalog_cli.rs:189` sets it `true` purely because `rebuild` returned `Ok`, with no check that the artifact is loadable or bound to the catalog generation it was derived from. Given the semantic packet is ongoing, either derive the field from the verified bundle or omit it from the report until the load path exists.

**P2-4 — The index store reuses catalog-store semantics for an unsigned, un-sequenced artifact.** `catalog_cli.rs:184-187` commits locally-generated bytes as another store's `active.bundle`. That record carries no publisher signature and no sequence, so `CatalogStore` provides atomicity but **no antirollback** for it: an older index for an older catalog can be substituted. ADR-041:82-83 requires mismatch detection to force lexical fallback — that must live on the load side, which is outside this packet. Flagging so it is not assumed complete.

**P2-5 — `--store` and `--index-store` are not required to be distinct.** Nothing at `catalog_cli.rs:184` compares the two roots. Committing index bytes over the catalog's `active.bundle` is prevented only incidentally, by `flock` conflicting between two open file descriptions on the same inode in the same process — surfacing as a confusing `CATALOG_BUSY`. macOS firmlinks make textually-distinct paths resolve to the same directory, so a string comparison is insufficient; compare the `Node` (dev, ino) of both roots explicitly.

**P2-6 — Key rotation is not survivable, contradicting ADR-041:20-21.** Because the floor is inside the active record, rotating the trust key makes the active bundle fail verification at `catalog_cli.rs:160-163`, bricking the store. The ADR's claim that "changing keys does not reset the stored sequence" is not implemented — in practice rotation forces store recreation, which resets the sequence to 0. Subsumed by the P1-2 fix.

**P2-7 — `UNIQUE = 0x2000` is unnamed, unprobed, and undocumented.** `catalog_store.rs:44` applies it via `from_bits_retain` to every non-directory open. `SAFE` is behaviorally probed at `:164-172`; `UNIQUE` is not, and XNU silently ignores unrecognized open flags — so whichever property it is meant to encode may simply be absent, with no test that would notice. Name it against the SDK header with a static assertion, or drop it.

**P2-8 — `RESOLVE_BENEATH` is anchored at `/` and therefore confines nothing.** `catalog_store.rs:155-180` opens the store path relative to a descriptor for `/`, so "beneath" is vacuous; the real controls are `NOFOLLOW_ANY` and the `..`-rejection in `path_checked`. That's a defensible design, but the comment at `:43` and ADR-041:13 read as if beneath-confinement is load-bearing. Either anchor beneath the store root for per-file opens or correct the comment.

**P2-9 — `status` is not read-only.** `CatalogStore::open` takes the exclusive `flock`, unlinks `staging.bundle`, and fsyncs the directory on *every* invocation including `status` (`catalog_store.rs:313-330`), and fails closed if a malformed staging file exists (`:344-346`, exercised at `tests/catalog_store.rs:302-311`). There is no shared/read-only open mode. **This needs a decision before the runtime packet lands:** if the MCP server holds a `CatalogStore` for its lifetime, every admin command returns `CATALOG_BUSY` permanently; if it does not, the runtime must re-verify on each read.

**P2-10 — Read→commit is not inode-bound.** `read_active` (`catalog_store.rs:352-363`) does not record the active `Node`, and `commit_checked` (`:379-384`) stamps the current active without comparing it to what was read. The staging path is meticulously inode-bound (`:399-408`); this window is guarded only by `flock` + the per-process `seen_active` flag. Not exploitable under the stated same-uid exclusion, but it is the one gap in an otherwise exhaustive TOCTOU chain.

**P2-11 — Minor:**
- `StoreError::Changed` maps to `SANDBOX_DENIED` with a permissions-flavored message (`catalog_cli.rs:212-215`), misleading for concurrent-modification/root-swap.
- `read_catalog_file` requires input files on APFS (`catalog_store.rs:79-91` via `:437`), so offline import from USB/exFAT/HFS+ DMG fails — awkward for an offline-acquisition story.
- `rustsec.json` has no per-file byte bound beyond the 80 MiB archive cap; `RustSecSnapshot::from_bytes` (`bundle.rs:244`) gets only a 60 s cooperative deadline. Post-authentication, so publisher-trusted, but ADR-041:91-93 lists no rustsec budget.
- `try_reserve_exact` inside chunked append loops (`bundle.rs:167`, `catalog_store.rs:265`) requests exact capacity per 64 KiB iteration; on macOS large-block realloc this risks quadratic copying across ~1280 iterations for an 80 MiB input.

---

## Test gaps (concrete)

1. **Multi-payload bundles are never constructed.** `signed_entries` (`bundle/tests.rs:113-129`) always produces exactly three members with one payload. That leaves *entirely untested*: the strictly-ascending `files` ordering rule (`bundle.rs:213`), the `files[0] == catalog.sqlite` positional assumption (`:225-232`), the whole `rustsec.json` branch (`:242-246`), and the semantic-index triple match (`:248-252`) including the `> 16 MiB` rejection and both half-declared cases `(Some, None, None)` / `(None, Some(1), Some(...))`. This is the largest gap in the packet and directly underpins P2-3/P2-4.
2. **Domain separation is untested.** No test signs the bare manifest without `SIGNING_CONTEXT` and asserts rejection — the one control the constant exists for.
3. **Manifest-side publisher/channel mismatch untested.** `tests.rs:154-165` mutates the *trust* struct; nothing mutates `publisher`/`channel` inside a validly-signed manifest to exercise `bundle.rs:195-197` from the attacker's side.
4. **Trust-file permissions have no test because the check does not exist** (P1-1).
5. **Fault injection is Rust-level only.** `catalog_store.rs:461-513` interrupts at two synchronous checkpoints; there is no test of `fsync`/`fcntl_fullfsync` returning an error (the `durable()` → `DurabilityUncertain` path at `:426`), and no real crash/power-loss evidence. The reported "deterministic faults" should be described as checkpoint injection, not crash testing.
6. **No cross-process commit concurrency test.** `concurrent_open_is_nonblocking_and_lock_releases_on_drop` (`tests/catalog_store.rs:87`) is same-process; the three CLI tests are strictly sequential. The lock-swap race handled at `catalog_store.rs:332-341` has no adversarial test.
7. **`rebuild-index` has zero coverage.** None of the three tests in `tests/catalog_cli.rs` exercise it, so `--model-dir`/`--index-store` parsing (`catalog_cli.rs:64-66`), the second-store commit, and the same-path case (P2-5) are unexercised.
8. **Untested `verify` branches:** `sequence > i64::MAX` (`bundle.rs:202`), `manifest.files.len() > MAX_FILES - 2` (`:204`), and the `MAX_FILES` boundary reached through `verify` rather than directly through `archive::entries`.
9. **Non-macOS fail-closed stubs** (`catalog_store.rs:520-541`) have a test but no stated CI evidence that a non-macOS target is actually built.

---

## Not reviewable in this packet

`crates/mcp-server/src/main.rs` was not provided, so I cannot confirm the `catalog` subcommand is unreachable from the MCP/stdin dispatch — `catalog_cli.rs:1` asserts it but the assertion is unverified here. Also outside the packet: `catalog_semantic::rebuild`, `semantic-adapter/src/index/`, `SqliteCatalogRepository::open`/`build` (taken as authoritative per the brief), `RustSecSnapshot::from_bytes` bounds, and whether `Provenance`'s `Deserialize` can construct values bypassing `Provenance::new` invariants — the latter is contained here by the equality check at `bundle.rs:237`, but worth confirming in the domain crate.
