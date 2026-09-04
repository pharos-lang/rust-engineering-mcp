# M1-01 prerequisite — owned source capture

Branch ai/m1-01-source-capture from clean main0d6462f. Captures selected project
subtree through existing original-root APFS no-follow handles, validates live
ProjectRef before/after, rechecks observed file/directory stamps and returns owned
bytes plus explicit/implicit directories. Empty directories survive. No project
code executes. This is not an atomic filesystem snapshot or Cargo containment.

ADR-031 bounds4096 entries, depth32,100-byte portable ASCII paths,1MiB per file,
16MiB total bytes. Reject links/hardlinks/nonregulars, .cargo configuration,
external/absolute Cargo paths and unsupported toolchain installation requests.
.git/target directories are excluded. Cargo include/exclude patterns accept only
literal paths; unsupported glob syntax fails closed. Captured standalone manifests
are checked too, including rejection of legacy aliases and replace. Manifest graph
validation reads captured bytes only. Cancellation does not renew ProjectRef TTL.

Tests include exact bytes, empty directories, collisions/bounds, live/stale/expired
refs, cancelled TTL, external dependency and absolute target/build paths, internal
workspace inheritance, source config, symlink/hardlink/FIFO/socket rejection and
regular-to-FIFO/hardlink replacement after directory enumeration. FIFO preparation
runs only in the integration harness; src contains no process creation.

Core final gate passed10/10 with 204 tests including doctest on this source-only
branch: Rust/Cargo1.98.1, CARGO_INCREMENTAL=0, locked/offline.
[Report](artifacts/M1-01-source-core.json). First core correctly rejected a test
mkfifo process in src; it was moved to integration without weakening architecture.
Principal diff review and [Opus5 High disposition](../reviews/M1-01-source-claude-opus-5.md)
completed. Five additional tests pin TOML parser size/depth, casing, inner-file
disappearance and pre-I/O path validation. Integration smoke is recorded separately. USTAR materialization and Rust gateway calibration remain separate.
M1-01 is not Done; only project.open is publicly operative.
