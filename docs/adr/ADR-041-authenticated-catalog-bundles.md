# ADR-041 — Authenticated catalog bundles and durable activation

## Status

Accepted for implementation, 2026-09-04. M1-10 remains in progress until evidence
exists. This decision does not approve an official publisher, license or release.

## Context

ADR-018 requires signed offline acquisition, bounded extraction and durable
antirollback. ADR-026 accepts only owned SQLite bytes with a trusted expected hash;
that is insufficient to authenticate distribution. The current host supports
no-follow handle I/O on macOS/APFS. Other hosts must fail closed.

## Decision

Implement an explicitly host-selected Ed25519 trust root, using ring 0.17.14 already
in Cargo.lock. No compiled public trust key, signing key generation, TOFU or implicit
trust in a bundle key. Trust configuration identifies one publisher/channel and
one public key in an owned mode-0600 file under an owned mode-0700 parent, with
ancestor replacement protection. Key rotation requires an explicit host change; changing keys does
not reset the stored sequence. Fixture keys are exclusively test identities.

The transport is bounded Zstandard-compressed strict USTAR, using zstd 0.13.3
already locked. Only regular files with canonical relative ASCII names are accepted;
links, devices, directories, extensions, sparse entries, duplicate paths, traversal
and unlisted members are rejected before any archive-selected filesystem write.
Metadata is a closed typed JSON manifest serialized in fixed struct-field order,
with no insignificant whitespace, followed by a detached Ed25519 signature over
the exact canonical bytes. It binds publisher/channel, monotonic sequence, format/
schema versions, SQLite identity, timestamps and every payload's length/SHA-256.
Cryptographic verification precedes payload parsing. No archive path is used for I/O.

Acquisition returns owned bounded bytes through the existing secure host filesystem
adapter. Staging is in memory for validation; only a complete validated bundle can
be committed to a private host-selected store. The durable active bundle is one atomic generation record. A separate bounded
`floor.record` reserves the highest authenticated sequence and exact container
hash before activation, under the same exclusive lock. Both records use separate
fixed staging names and file/directory full durability. The floor binds publisher
and channel, independently of the current signing key. A failed activation can
retry that exact reserved container; a different container requires a higher
sequence. A healthy active generation still rejects equal imports. Invalid or
missing active data can be repaired from the exact reserved container or a newer
signed generation without deleting the floor. Corrupt/missing floor beside an
existing active record fails closed; no implicit reset or reconstruction occurs.
An exclusive OS lock serializes imports. Status exposes active sequence, reserved floor sequence/hash and a pending
reservation flag, so interrupted activation is observable. Trust channel mismatch,
invalid floor and active data unverifiable under current key have distinct errors.
An existing invalid active record makes status/rebuild fail closed; import may
repair it only after independently validating the retained floor. Recovery never
falls back to an older bundle. A partial staging record is
discardable only under that lock, using fixed names and the original directory
handle. Atomic replacement and file/directory durability precede success. OS I/O
failure after replacement reports uncertain durability, never success. The trusted
host must preserve its state; deliberate deletion/restoration of the entire trusted
store by its owner is outside software antirollback guarantees.

The runtime consumes a verified active snapshot and never synchronizes or downloads.
CLI acquisition and rebuild are explicit. Existing SQLite schema/ledger validation
and budgets remain authoritative. Unknown schema versions fail without modifying
the active generation; no imported SQL migration is executed. Migrations remain
compiled, transactional and monotonic. Global catalog capacity remains ADR-026's
bounded dataset, not a claim of complete crates.io coverage.

## Alternatives considered

- TLS/checksums alone: do not authenticate publisher or prevent replay after restart.
- Extract tar to disk then validate: gives untrusted metadata filesystem authority.
- Unordered independent pointer/floor writes: introduces unsafe mixed generations.
  Reserve-before-activate ordering instead permits a floor ahead of active, never
  behind it; interrupted reservations may retry only their exact authenticated bytes.
- Open SQLite/LanceDB by canonicalized paths: bypasses the handle confinement model.
- Publish fixture keys as product keys: invents distribution authority.

## Consequences

Import mechanics can be tested independently of an approved official publisher.
Public sync and release still require actual source/licensing/trust decisions.
An interrupted import yields either the previous complete generation or the next
complete generation; callers must reread status after uncertain commit. No downgrade
flag is introduced in this cut. Signed format details, resource budgets, persistence
tests and independently reviewed evidence must accompany implementation.

Sources: https://docs.rs/ring/0.17.14/ring/signature/,
https://docs.rs/zstd/0.13.3/zstd/stream/read/struct.Decoder.html,
https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/sys/fcntl.h,
https://www.sqlite.org/atomiccommit.html.

### Derived index persistence

Persist the actual native Lance 8 object set in a bounded owned-byte artifact,
including metadata and per-object hashes. The CLI writes the artifact through
host directory handles. Lance never receives a filesystem URI from that artifact:
restore bytes into a fresh in-memory object store whose registry has only the
memory provider, then open the existing native table and validate all rows/schema.
This is persistence of native objects, not reconstruction from synthetic vectors.
A generation is immutable while exported. Missing, corrupt or mismatched derived
artifacts cause explicit lexical fallback; they cannot overwrite SQLite facts.
The existing memory-only model byte loader remains; the public runtime has no
acquisition authority. This extends ADR-027 for M1; it does not claim an ANN index
or hard native allocator resource limits. Native object lists/bytes and embedding
identity must be bounded and validated before opening the table.

### Concrete budgets and local model acquisition

The compressed input and decompressed USTAR each have an 80 MiB cap, at most16
regular entries, a16 KiB manifest and an8 MiB Zstandard window. SQLite retains its
64 MiB/1000-crate budget. Native index artifacts have16 MiB total,128 objects and
8 MiB per-object limits. Fixed E5 filenames are acquired by a separate no-follow
model reader, maximum512 MiB per file and exact E5_FILES lengths/hashes before
native parsing. This reader does not raise the catalog/store80 MiB limits. Reads
check a60s cooperative deadline between chunks; kernel stalls/native calls cannot
be forcibly interrupted in-process. No OS-level total RSS or hard CPU bound is
claimed. New input acquisition occurs only in explicit CLI or host-configured
runtime loading, never through an agent-selected path or implicit home lookup.

### Explicit network acquisition

The optional CLI sync acquisition accepts only a host-selected HTTPS URL and exact
allowed hostname, with no credentials, fragments, non-443 port, proxy inheritance,
redirects, automatic content decompression or retries. Use reqwest0.12.28 already
locked with explicit rustls/webpki roots and no default features; fixed user agent,
connect/overall deadlines and streamed compressed80 MiB bound. This is an explicit
CLI network operation, never callable by runtime tools. The network response is
still untrusted until the same publisher signature and rollback checks pass. There
is no built-in public endpoint or approved distribution/source identity. Local
mirror sync continues to work offline; source terms and publisher approval remain
required before official distribution. No TLS test key is a publisher trust root.
