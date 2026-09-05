# M1 closure release artifact — Claude Sonnet 5

Date: 2026-09-04. Scope: `scripts/release-artifact.py`,
`scripts/test-release-artifact.py`, `scripts/release-smoke.py` and
`scripts/test-release-smoke.py`. All invocations were read-only, explicitly selected
`claude-sonnet-5`, disabled tools and MCP, used no session persistence, and reported
only `claude-sonnet-5` in `modelUsage`.

## Review trail

1. High-effort review rejected the first cut because guessed filenames such as
   `license.rs` could satisfy the license-text gate, the packager did not bind the
   host or Mach-O architecture, owner metadata and deterministic scope were
   incomplete, partial publication cleanup was weak, and a hash input handle was
   not explicitly closed. Session `261627e1-7535-4e93-b068-a41d165900ab`, result
   UUID `cc64dd79-5bbf-4f98-835d-aebe877b2ad8`.
2. High-effort follow-up confirmed most fixes but rejected an arbitrary dash-suffix
   filename path such as `LICENSE-vendor.rs`. It also proposed P2 hardening for host
   validation in the smoke, `MH_EXECUTE`, prohibited package prefixes, notice
   revalidation and runtime coverage. Session
   `a56aa4a8-37f0-4cbd-a7f1-65a82f11b937`, result UUID
   `a43f75cf-a4ed-4625-9d15-06718ba37074`.
3. Medium-effort follow-up found a second apparent P1 around Cargo's
   `license_file`, because it is not filename-filtered. Session
   `320bcef2-e2ed-4bb0-afe3-1a52a980fc53`, result UUID
   `2f9979b7-281b-4822-8c0e-6efc4cddd10d`.
4. The final focused medium-effort follow-up **accepted** the explicit distinction:
   discovered names use a closed allowlist, while Cargo's publisher-declared
   `license_file` is contained inside the package root and records separate
   `manifest-license-file` provenance. It found no remaining P0/P1. Session
   `dccc276d-3e18-489d-82e9-1b79e216c9d8`, result UUID
   `20eecb42-7529-4b5d-a843-9aca92053d92`.

## Principal disposition

- P0/P1: all resolved and independently accepted.
- Host/Mach-O P2: resolved by requiring Darwin ARM64 for packaging and smoke and by
  verifying thin `MH_EXECUTE` Mach-O ARM64 bytes in both paths.
- Prohibited-prefix P2: resolved in both producer and independent consumer.
- Exact notice-byte parsing P2: accepted as bounded. Every included text byte string
  is generated from the locked source tree, hashed in the inventory, embedded in the
  notices, then the entire archive is bound by `SHA256SUMS`, the manifest, and the
  smoke's archive hash before any process runs. A substring check is not the sole
  integrity control.
- Nine tools not invoked by the artifact smoke P2: accepted for this cut. The smoke
  freezes all 26 input/output schemas and calls four discriminating unavailable and
  deny paths; the full source gate and candidate-bound Inspector/model sessions are
  separate required closure gates for runtime behavior.
- Additional suggested duplicate-key/symlink tests: the strict JSON hook rejects
  duplicate keys and the tar reader rejects every non-regular type in the same
  branch. These are defense-in-depth follow-ups, not open release findings.

Focused executable evidence at acceptance: 11 artifact tests and 9 smoke tests pass;
the exact target-filtered offline closure contains 219 packages and 549 dependency
edges with no missing exact license text.
