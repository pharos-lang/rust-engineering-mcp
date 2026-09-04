# Read-only security review — M1-10 native persistence, E5 rebuild, explicit HTTPS

Scope: the five files supplied. `catalog_cli`, `stdio`, `capabilities`, `bundle`, `catalog_store`, `VerifiedE5Bundle`, and `lancedb` internals were **not** in the packet; findings that depend on them are marked *unverifiable in packet*. I did not execute anything.

## P0

None substantiated in the supplied code. The archive framing, path canonicalization, identity binding, and the HTTPS authority check all fail closed under the cases I traced.

## P1

**P1-1 — `active.bundle` is unauthenticated input to native Lance parsers**
`crates/mcp-server/src/catalog_semantic.rs:124-141`

`validate_persisted_index` reads `index_path.join("active.bundle")` and hands the bytes straight to `LanceMemoryIndex::restore`. The only integrity binding is `decode`'s per-object SHA-256 (`persistence.rs:129`), which is computed over the same untrusted bytes and is therefore self-authenticating — an attacker recomputes it trivially — plus exact equality against `metadata_bytes(expected)` (`persistence.rs:100`), which an attacker can also reproduce since it is derived from the SQLite snapshot fingerprint and the locally installed E5 identity, both readable locally.

*Precondition:* write access to the `--index-store` directory (same-UID process, backup restore, shared/synced dir, or a container volume). No key material or network position needed.
*Impact:* fully attacker-chosen protobuf/Arrow/Lance-page bytes reach native decoders before any row-level validation (`open_table` at `persistence.rs:242` precedes `validate_table` at `:247`). Post-parse damage is bounded — `validate_table` re-derives `crate_names` from the table and `catalog_semantic.rs:149` requires exact equality with SQLite's document names — so crate *identity* cannot be injected, but every **embedding vector is attacker-controlled**, i.e. arbitrary reordering/poisoning of semantic candidate ranking while SQLite facts stay honest. Contrast with the import path (`catalog_semantic.rs:90`), where bytes come from `VerifiedBundle` and are publisher-signed.
*Note:* the stated boundary ("authenticated publisher") holds for import but **not** for the on-disk persisted bundle, unless `catalog_cli` records a digest of `active.bundle` in the authoritative SQLite store — not visible in this packet.

**P1-2 — row-count gate runs after the expensive work in `rebuild`**
`crates/mcp-server/src/catalog_semantic.rs:33-41` (gate is `index.rs:47-49`)

`embedding_documents()` is fully materialized, `Vec::with_capacity(documents.len())` is allocated from that count, and one `embed_passage` runs per document. `MAX_ROWS = 1000` is only enforced later inside `LanceMemoryIndex::build` (`index.rs:47`), i.e. after every embedding has been computed and retained.

*Precondition:* a catalog whose `embedding_documents()` returns ≫1000 rows — reachable from an oversized but correctly signed publisher bundle, which the trust boundary permits.
*Impact:* one `with_capacity` sized by an untrusted row count; N model inferences and N×dimension f32 retained, all discarded by a `Budget` error that could have been returned in O(1). `rebuild_budget` (`:164-166`) is cooperative wall-clock only: it is not checked inside `load_model` (`:31`, unbounded) nor inside a single `embed_passage`, so it bounds neither the allocation nor any individual call. Accepting the packet's "cooperative limits, no hard RSS claim" caveat, this is still a check-ordering defect rather than a limits question.

## P2

**P2-1 — `E5_FILES` ↔ verifier arity is an unchecked binding.** `catalog_semantic.rs:16-17`. `files: [Vec<u8>; 5]` zipped against `E5_FILES`; `zip` truncates silently. A sixth `E5_FILES` entry would never be read or hashed, and a fifth-only-in-array case leaves an empty `Vec`. Fail-closed today (fixed hashes in `VerifiedE5Bundle::verify` reject empties), but nothing enforces the arity at compile time. *Unverifiable in packet: `E5_FILES` length.*

**P2-2 — manifest validation does not cover the index section or version chain.** `persistence.rs:265-276` checks `base_paths`, fragment count, `deletion_file`, `base_id`, and `files[].path`. It does not inspect secondary-index metadata, the `_versions` chain, or reader/writer feature flags. Currently mitigated only by `bypass_vector_index()` at `index.rs:128`; removing that line (e.g. a future ANN optimization) silently converts unvalidated index references into a live read path.

**P2-3 — `dimension as i32` truncation, inconsistent with the sibling.** `persistence.rs:282` uses an `as` cast where `index.rs:58` correctly uses `i32::try_from`. Not exploitable today (`model.validate()` runs at `persistence.rs:51` before this point and caps dimension ≤ 1024), but the invariant is enforced at a distance.

**P2-4 — identity binding depends on `serde_json` byte-determinism.** `persistence.rs:100` compares serialized bytes rather than the deserialized value. Adding a map-typed field to `IndexMetadata` would produce nondeterministic ordering and spurious `IdentityMismatch`. Fail-closed, but brittle.

**P2-5 — Session cache units unverified.** `persistence.rs:18-22` passes `8 * 1024 * 1024` twice to `lancedb::Session::new`. If either parameter is an entry count rather than a byte budget, the cache is effectively unbounded. Worth confirming against the pinned lancedb version.

**P2-6 — registry containment is tested only at the registry API, not through a manifest-referenced absolute URI.** `persistence.rs:472-489` asserts `get_store` rejects `file://`, `s3://`, `https://`, `shared-memory://`. The manifest-mutation test (`:528-564`) reaches rejection via `canonical_path` (`:271`), because `:` and `..` are excluded — i.e. it proves the *string* filter works, not that Lance would refuse to resolve an absolute URI through a non-session registry. The "no arbitrary I/O" claim therefore still rests on lancedb never falling back to a default/global registry.

**P2-7 — `--root` accepts relative paths and duplicates.** `main.rs:81`. `--rustsec-snapshot` requires `path.is_absolute()` (`:95`) but `--root` does not, so roots resolve against the server CWD. Inconsistent binding between two host-authorization flags. (The overflow/non-UTF-8 behavior is correct: a 17th `--root` falls through to `else { Unsupported }` at `:121-123` rather than being ignored.)

**P2-8 — `--docker` executable is not required to be absolute.** `main.rs:108-120`. Only UTF-8 validity and non-duplication are checked; whether a relative value is resolved via `PATH`/CWD depends on `rust_engineering_execution` (*unverifiable in packet*). Note `--rust-image` is correctly pinned to `APPROVED_RUST_IMAGE` at `:135`.

**P2-9 — network capability has no compile-time gate.** `main.rs:10` declares `mod catalog_sync` unconditionally, while the heavy semantic paths are `#[cfg(feature = "local")]`. The TLS stack and `SyncSource` are linked into every build including `serve --stdio`. The claim "CLI explicitly network-enabled, only sync-remote" is enforced entirely inside `catalog_cli`/`stdio`, neither of which is in this packet — I can confirm only that `SyncSource::new` requires a host-supplied `allowed_host` and that `fetch_response`'s budget/timeout overrides are private (`catalog_sync.rs:126`) and unreachable from `SyncSource::fetch` (`:71`), which uses `MAX_BYTES`/`OVERALL_TIMEOUT`.

**P2-10 — `Content-Encoding` comparison is exact-bytes.** `catalog_sync.rs:141-146` rejects anything not literally `identity`, including `IDENTITY`. Fail-closed (availability only). Separately, no `min_tls_version` pin — rustls defaults to TLS 1.2+, acceptable, but not asserted.

**P2-11 — nested `block_on` panics if ever called from async context.** `catalog_semantic.rs:47`, `:86`, `:135` each build a current-thread runtime inside a sync fn. Safe from a sync CLI entrypoint; a panic (not an error) if any of these is later reached from a tokio worker.

**P2-12 — `rebuild` holds two generations concurrently.** `index.rs:104-105`: `Self::build` completes before `*self = candidate`, so both memory stores and both session caches are live at the swap. Correct for the stated atomicity goal (failure leaves the old generation intact), but peak is ~2×; worth stating explicitly rather than leaving implied.

**P2-13 — vectors are never bound to SQLite documents.** `catalog_semantic.rs:93-103` and `:142-151` compare sorted `crate_names` only. An index whose vectors are unrelated to the documents passes. Within the publisher boundary for import; combined with P1-1 this is what makes the persisted-bundle path exploitable for ranking manipulation.

**P2-14 — full model buffered for an identity-only read.** `load_model` (`:10-24`) reads all five E5 files and initializes the ORT runtime; `validate_imported_index` (`:80`) and `validate_persisted_index` (`:129`) use the result only for `provider.identity()`. Per-file bounds come from `E5_FILES` sizes; there is no aggregate cap and no budget around the load.

## Checked and holding

Worth recording so these are not re-reviewed: URL authority handling in `catalog_sync.rs:33-65` resists the cases I could construct — raw-authority comparison before `Url::parse` defeats userinfo (`host@evil`, `host:443@evil`), percent-encoded dots, uppercase scheme, and backslash; `!url.is_ascii()` (`:35`) precedes any IDN/punycode normalization; numeric-alias hosts (`2130706433`, `0x7f000001`, `127.1`) are caught by the raw-vs-normalized `host_str` mismatch at `:59` even though `IpAddr::parse` at `:60` does not recognize them; `:0443` fails `strip_suffix(":443")`. `decode` (`:88-141`) enforces strict ascending object order (`:119`), so duplicates are impossible, and rejects trailing bytes (`:137`). `search` (`index.rs:141-177`) constrains per-batch and total row counts, set-membership against the validated `crate_names`, and monotonic non-negative finite distances. `fetch_response` (`:154-163`) bounds the streamed body identically to the declared length and uses `try_reserve_exact`. `#[cfg(not(feature = "local"))] validate_imported_index` (`:107-116`) correctly rejects bundles carrying index bytes rather than passing them through.
