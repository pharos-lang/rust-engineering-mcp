# Catalog bundles and explicit CLI — M1-10

Implementation follows [ADR-041](adr/ADR-041-authenticated-catalog-bundles.md).
This is development documentation; see [M1-10 evidence](validation/M1-10.md) for
current gates, review disposition and local integration.
No production catalog publisher, model redistribution or catalog release is approved.
ADR-047's source publisher and GitHub channel do not select an Ed25519 catalog key.
The ten operative MCP tools are unchanged. CLI acquisition cannot be called by a
tool; the runtime never synchronizes or downloads catalogs, advisories or models.

## Commands and paths

```text
rust-engineering-mcp catalog status --store PATH --trust PATH [--model-dir PATH [--index-store PATH]] [--json]
rust-engineering-mcp catalog import SNAPSHOT --store PATH --trust PATH [--model-dir PATH] [--json]
rust-engineering-mcp catalog sync --source SNAPSHOT --store PATH --trust PATH [--model-dir PATH] [--json]
rust-engineering-mcp catalog sync --url HTTPS_URL --allow-host HOST --store PATH --trust PATH [--model-dir PATH] [--json]
rust-engineering-mcp catalog rebuild-index --store PATH --trust PATH --index-store PATH --model-dir PATH [--json]
```

Paths are absolute, physical and host-selected; do not use `/tmp` when it resolves
through a symlink. Store/index-store directories must already exist, be owned by
the current user and have mode0700. Use distinct directories. Files use no-follow
handle reads and regular/single-link checks on validated macOS26+/APFS. Other
platforms/filesystems fail closed, including input media lacking these guarantees.
Arguments are closed: duplicate flags, source plus URL, URL without allowed host,
relative paths and index-store on import/sync are rejected. Status accepts model
alone for an embedded index; an external index-store additionally requires model.
Import of a semantic payload requires the verified model.

`--trust` selects a bounded host-owned mode0600 JSON file under a mode0700 parent:
`publisher`, `channel`, `public_key` (64 lowercase hex characters, raw Ed25519).
No bundle-supplied key is trusted. Ancestors must be root/current-user owned and
not group/other writable, with the implementation's narrow root-owned sticky
`/private/tmp` exception immediately followed by a private user directory.
Ownership/mode and ancestor identity are checked; the host must provision these
paths without ACL grants that undermine that policy. ACL absence is not enforced
by the POSIX-mode checks. Never use the public fixture identity for real data.

Local sync reads a pre-acquired bundle through the same verifier as import. Remote
sync requires exact lowercase DNS hostname matching, HTTPS/443, no userinfo or
fragment, and URL length at most2048 bytes. It disables proxies, redirects,
retries and HTTP content decompression; only200 with absent/identity encoding is
accepted. TLS uses rustls/webpki roots. Connect deadline10s, overall transfer60s,
streamed compressed limit80MiB. This is explicit network acquisition, not an
enforced network-deny operation; downloaded bytes still need publisher verification.
There is no built-in endpoint or approved global catalog source.

## Signed transport v1

The archive is Zstandard-compressed strict USTAR: first `manifest.json`, then
`signature.ed25519`, then the signed payload list in strictly ascending path order.
Payload names are limited to mandatory `catalog.sqlite`, optional `rustsec.json`
and optional `semantic.index`. Names never become filesystem write paths. Only
regular entries, bounded ASCII octal metadata, checked checksums/zero padding and
zero archive termination are accepted; links, directories, devices, extensions,
traversal, duplicates, omitted/unlisted payloads and reordering are rejected.

`BundleManifest` in `crates/catalog-adapter/src/bundle.rs` is the serialization
authority. Canonical JSON is exactly `serde_json::to_vec` of the typed manifest:
fixed struct-field order, no insignificant whitespace, no unknown fields, and
explicit null Options. This is not a general JSON canonicalization standard.
Top-level order is `snapshot_format_version`, `catalog_schema_version`,
`semantic_index_version`, `embedding_model_id`, `publisher`, `channel`, `sequence`,
`catalog_provenance`, `files`. File rows contain `path`, `byte_length`, `sha256`.
Nested provenance uses its Rust serialization. Supported format/schema versions
are1 and sequence is1..i64::MAX.

The signature is64 raw Ed25519 bytes over the concatenation of ASCII
`rust-engineering-catalog-bundle-v1`, one NUL byte and the exact manifest bytes.
Signing bare JSON is invalid. Signature verification precedes manifest/payload
parsing; publisher/channel must match host trust. Every payload length and SHA256
must match. SQLite schema/ledger/provenance and optional RustSec document are then
validated; no bundle-provided SQL migration executes. Import does not refresh
source timestamps. A semantic member requires version1 and model ID
`intfloat/multilingual-e5-small`; transport validation alone does not make it usable.

## Durable state and derived index

The catalog store serializes operations using `store.lock`. It owns fixed names
`active.bundle`, `staging.bundle`, `floor.record`, `floor.staging`. The independent
floor binds publisher/channel, highest reserved sequence and exact container hash;
its checksum detects corruption, not a malicious trusted owner. Reserve the floor
durably before activating the validated bundle; each replacement requires file and
directory durability. Success also checks exact active-byte readback.

After interrupted activation, the exact reserved container or a newer signed
generation may repair missing/invalid active data. Healthy active data still
rejects equal imports. A corrupt floor, or a missing floor beside existing active
data, fails closed: do not delete records to recover. Key changes do not reset
the floor; new bundles must still verify under the explicit current trust key.
An I/O failure after replacement reports uncertain durability; reread status.
Status acquires the exclusive lease and may clean fixed staging files; it is not
a lock-free read. Deliberate whole-store deletion/restore by its owner is outside
software antirollback guarantees. A pending reservation is visible as `floor_sequence`, `floor_bundle_sha256` and
`reservation_pending`, including human output. A changed publisher/channel reports
`CATALOG_TRUST_MISMATCH`; unverifiable active data reports
`CATALOG_ACTIVE_UNVERIFIED`. Key rotation requires a newer bundle signed by the new
key; it does not make the retired-key container verifiable.

Rebuild requires feature `local`, the five exact E5 files and approved native ORT.
It embeds authoritative SQLite documents, exports actual Lance8 objects and
reopens them in a fresh memory-only object store before persisting. Restore checks
metadata, complete model identity, catalog fingerprint, object hashes/native table
schema and crate-name coverage. No artifact path becomes a Lance filesystem URI.
Imported indexes undergo native restore before activation; missing/invalid required
model/index prevents import. Status with model alone validates the embedded index;
with index-store it prefers that external derived artifact. Without native
validation, or on invalid derived data, it reports semantic availability false
while retaining catalog facts. This does not implement the pending MCP search tool.

## Budgets and report

Compressed and decompressed bundle:80MiB each; Zstd window8MiB; at most16 entries;
manifest16KiB. SQLite retains64MiB/1000-crate bounds. Native index:16MiB total,
128 objects,8MiB/object,16KiB metadata. Model acquisition has a separate512MiB/file
ceiling and exact pinned lengths/hashes. Reads check60s between chunks; bundle
decompression checks30s; rebuild checks300s between model/inference/native steps.
These are cooperative deadlines, not hard native CPU/RSS/I/O containment. Several
owned/native buffers can coexist; the byte caps are not an aggregate RSS promise.

JSON stdout is one line with `format_version:1`, `status` (`passed`/`unavailable`),
`operation` (`status`/`import`/`sync`/`rebuild-index`), nullable `error_code`, fixed
`message`, `network_used`, nullable `catalog`. Exit0 means completed success,1
operational failure,2 invalid invocation. `network_used` means remote acquisition
was attempted, including failed transfers, not that data was accepted.

Catalog fields: `semantics:"latest_known"`, `publisher`, `channel`,
`publisher_key_sha256`, `sequence`, `floor_sequence`, `floor_bundle_sha256`,
`reservation_pending`, `bundle_sha256`, `catalog_sha256`,
`schema_version`, snapshot `evidence`, `rustsec_available`,
`semantic_index_available`. Freshness uses source timestamps and
`catalog-snapshot-v1` (fresh86400s, aging through604800s), never import time.
`bundle_sha256` identifies exact compressed container bytes; equivalent recompressed
archives can differ. Compare catalog fingerprint, sequence and publisher/channel
for catalog facts; retain exact container bytes for reserved-generation recovery.
Errors include `CATALOG_ROLLBACK`, `CATALOG_STATE_INVALID`, `CATALOG_STATE_CHANGED`,
`CATALOG_DURABILITY_UNCERTAIN`, `CATALOG_BUSY`, `CATALOG_UNTRUSTED_PUBLISHER`,
`CATALOG_INVALID_SIGNATURE`, `CATALOG_ACTIVE_UNVERIFIED`, `CATALOG_TRUST_MISMATCH`,
`CATALOG_UNSUPPORTED_SCHEMA`, `CATALOG_INVALID_BUNDLE`,
`CATALOG_UNAVAILABLE`, `SEMANTIC_REBUILD_UNAVAILABLE`, `OUTPUT_LIMIT_EXCEEDED`,
`UNSUPPORTED_PLATFORM`, `SANDBOX_DENIED`, `CATALOG_IO_ERROR`, `NETWORK_DENIED` and
`CATALOG_SYNC_UNAVAILABLE`. Fixed messages omit untrusted paths and network details.
