# Signed catalog development fixtures

`fixture-1.tar.zst` and `fixture-2.tar.zst` contain a real bounded SQLite catalog
with sequences1/2, publisher `fixture-only`, channel `test`. The trust JSON contains
the public key derived from the deliberately public Ed25519 seed `[42; 32]` in
`crates/catalog-adapter/src/bundle/tests.rs`. Anyone can sign with this identity:
it grants no production or distribution authority. No external publisher/license
approval follows from fixture verification.

The catalog contains illustrative serde1.0.0 metadata and source timestamps100
seconds after the Unix epoch. Preserve them: imports should report stale source
evidence, not renew freshness. These files do not represent current crates.io,
a complete registry, or a useful retrieval benchmark. The fixtures have no native
index/model payload; those paths use separate real-model tests.

Tests copy the trust bytes to an owned mode0600 file in a private mode0700 APFS
directory with protected ancestors. The checkout's trust file is not automatically
a valid operational trust path. Store directories must be created explicitly.
Never modify fixtures during normal test runs. The ignored
`emit_development_fixtures` test is an explicit maintainer-only emitter, not a
publisher CLI, and remains excluded from the ordinary gate.

See [bundle format and CLI](../../docs/catalog-bundle-format.md) and ADR-041.
The final floor/recovery/full gate remains separate from historical fixture passes.
