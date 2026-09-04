# ADR-027 — Verified local embeddings and memory-only LanceDB generations

## Context

M0-09 needs actual E5 inference and LanceDB retrieval under offline enforcement.
Experiments found lancedb0.38.0 fails without remote and Lance11 eagerly creates a
filesystem spill directory even for memory://. lancedb0.31.0/Lance8 has neither
problem, but mistakenly places lance-testing in normal dependencies, importing a
Linux profiler and vulnerable quick-xml. Its three references are all cfg(test).

## Decision

Pin lancedb0.31.0, vendor the published crate (SHA256
2bd0b54bb1cdd075efa5a8827ec16dcf5c0781253cd88e63988c174915c53fe2), and change only
Cargo.toml and Cargo.toml.orig to move lance-testing into dev-dependencies. Exclude
vendor from workspace membership. Preserve every Rust source byte; keep an exact
patch and verification script. This removes lance-testing/pprof/inferno/quick-xml
from the application graph without enabling remote or suppressing advisories.
Pin tinyvec1.12.0 in the lock:1.13.0 fails compiling the alloc macro path in this
graph. Record paste1.0.15/RUSTSEC-2024-0436 as an unmaintained transitive macro crate;
no vulnerability ignore is authorized by this decision.

Use a fresh memory:// database per generation, retaining its connection/table
handles. No filesystem URI, shared-memory global pool, remote endpoint or spill
store is accepted. Metadata binds index schema, snapshot fingerprint and complete
embedding identity. Build a new generation from verified embeddings then replace
it atomically; invalid/missing/mismatched generation causes declared lexical
fallback. SQLite remains authoritative for every returned fact. A reconstructed
in-memory index is sufficient for M0; persisted index distributions are M1 work.

Pin fastembed6.0.2 with defaults disabled (no hf-hub or ORT downloads), local CPU
user-defined ONNX files only. Accept only the five exact size/hash-verified E5 files
from immutable revision614241f622f53c4eeff9890bdc4f31cfecc418b3. The adapter receives
owned bytes, never opens model paths. Use mean pooling, query:/passage: prefixes,
512 tokens, whitespace-normalized passages, two intra-op threads and normalized384-dimensional vectors. Native ORT
is explicitly supplied by the host at build time; local validation identifies the
installed static1.24.2 artifact by SHA256, not a newly authenticated distribution.
Production distribution/runtime provenance remains a release gate.

The development feature `local` includes the native semantic adapters. Core-only
builds exercise absence/fallback but cannot qualify as M1 releases. Full M0 semantic
gate explicitly enables local, provisions no files during runtime, and runs real
inference/table creation/query under a network-deny profile with calibrated controls.
A missing build-time ORT library fails rather than downloading. The future MCP
integration must use a bounded blocking worker for CPU inference; no new tool is
announced by this foundation API.

## Alternatives considered

- lancedb0.38 with remote enabled: does not address eager filesystem I/O and adds
  unnecessary surface merely to avoid an upstream compilation defect.
- Patch Lance11 constructors and0.38 jobs: more code/dependency ownership than a
  manifest-only correction to a working published version.
- Keep vulnerable test-only profiler in runtime or ignore advisories: unnecessary.
- Fake embeddings or a custom nearest-neighbor store: would not prove the required
  fastembed/LanceDB boundary.

## Consequences

Vendor adds about2.3MB/93 published files; maintain the patch until upstream permits
safe adoption of a newer release. Benchmark/cross-platform and ES/EN quality gates
remain mandatory before M1 RC. Memory generations and verified bounded inputs limit
surface, not aggregate native allocator RAM or hard CPU deadlines. Network-deny
calibration is not an assertion of the product's full strict sandbox tier.

## Status

Accepted and implemented in M0-09. See validation/M0-09.md and the principal
disposition of the independent Opus5 review.

Sources: https://github.com/lancedb/lancedb/tree/v0.31.0/rust/lancedb,
https://github.com/lance-format/lance/blob/v8.0.0/rust/lance/src/session.rs,
https://github.com/Anush008/fastembed-rs/tree/v6.0.2,
https://huggingface.co/intfloat/multilingual-e5-small/tree/614241f622f53c4eeff9890bdc4f31cfecc418b3.
