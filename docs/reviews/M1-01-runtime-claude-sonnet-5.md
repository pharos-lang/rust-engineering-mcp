# M1 runtime provisioning — external review disposition

Claude Code2.1.259, explicit claude-sonnet-5, medium effort, read-only supplied
packet, tools disabled/safe/restricted mode and no session persistence.
Actual modelUsage confirms Sonnet5; auxiliary Haiku telemetry also present.

Principal disposition:

- P1 Config equality: rejected as contradicted by actual Docker29.7.2/API1.55
  inspection. The final scratch image Config has exactly the three expected keys.
  The unchanged strict equality check passed an actual eight-command verification
  replay. The persisted image receipt proves it. Do not weaken this check to ignore
  unexpected execution configuration based on historical API assumptions.
- P2 extraction: trusted pinned official installers are intentionally executable
  during explicit provisioning. Archive checksums are verified before extraction;
  these are not untrusted project bundles. No sandbox certification inferred.
- P2 versions: preserved verbatim official manifest metadata; component versioning
  differs from the runtime Rust/Cargo release. Actual executable versions are
  independently checked. No substitution occurred.
- No confirmed blocking defect remains in this provisioning unit. Profile/source
  and hostile Cargo calibration are separate unfinished prerequisites.

## Independent reviewer output

## Review: `fixtures/rust-runtime/*` (host-only Rust/Cargo 1.98.1 ARM64 provisioning)

Scope acknowledged: this is explicit, owner-authorized host provisioning tooling, not the MCP runtime, not a production sandbox/gateway, and not a reproducible-build or detached-signature claim. Findings below are scoped accordingly; I've separated genuine bugs from documented scope limitations.

### P1 — `verify.py:20-22` likely-broken Config equality check

```python
expected = {'User': '65534:65534', 'Env': [...], 'WorkingDir': '/work'}
if inspection['Config'] != expected or ...
```

`docker image inspect` always returns a full `Config` object (`Hostname`, `Cmd`, `Entrypoint`, `Volumes`, `Labels`, `ArgsEscaped`, etc., even when null/unset). Comparing the *entire* `Config` dict for equality against a 3-key subset will almost certainly never match, since the real object has additional keys the `expected` dict lacks. As written, this check should raise `SystemExit` on every run — which contradicts the README's claimed successful verification output (lines 44-49 of README). Either:
- the code that actually produced that README result differs from what's committed, or
- the check has never actually been exercised successfully.

This fails *closed* (blocks use rather than silently accepting a tampered config), so it's not a security hole, but it is a functional defect that should be fixed to compare only the intended subkeys (`inspection['Config']['User']`, `['Env']`, `['WorkingDir']`) rather than the whole dict.

### P2 — Extraction lacks path-traversal/ownership hardening

`Dockerfile:5`:
```
for archive in *.tar.xz; do tar -xJf "$archive"; done
```
No `--no-same-owner` or path-safety flags are used when extracting the hash-verified archives. Given the archives are pinned by SHA-256 in `sources.json` and trust is already anchored there, this is a defense-in-depth gap rather than an exploitable bug — worth a belt-and-suspenders `tar --no-same-owner` for portability, but not a security defect in this context.

### P2 — Version metadata inconsistency in `sources.json`

`sources.json:23,30,37`: `manifest_version` for `cargo` (`"0.99.0..."`), `rustfmt-preview` (`"1.9.0"`), and `clippy-preview` (`"0.1.98"`) don't match the pinned `rust_version: "1.98.1"`. This reflects Rust's real independent component versioning scheme (cargo/rustfmt/clippy ship their own internal version numbers distinct from the rustc release) rather than a bug — `verify.py:55-56` correctly asserts against the authoritative runtime output (`rustc 1.98.1`, `cargo 1.98.1`) rather than trusting this metadata. Flagging only for documentation clarity, since a reader could misread it as a mismatch.

### Not findings — explicitly documented scope limitations (correctly handled, no action needed)

- `Dockerfile:9` — final `ENV PATH` intentionally omits `/opt/rust/bin`; README explicitly states callers must use fixed tool paths and rebuild PATH themselves. Intentional, not an oversight.
- `README.md:24-26` — apt index mutability affecting reproducibility is called out; the recorded immutable local image ID is correctly positioned as the source of truth, not the mutable tag. Consistent with "not a reproducible apt claim" framing in the task.
- `verify.py:24-29` — hardened container run (`--network none`, `--cap-drop ALL`, `--read-only`, resource limits, noexec tmpfs) is explicitly scoped as verification-only, not sandbox/gateway certification (README:40-42 says as much). No seccomp profile is applied beyond Docker defaults — consistent with README's stated deferral of "seccomp calibration" to separate owner-controlled work, not a gap in this fixture's stated purpose.
- `provision.py:16` / `verify.py:15-16` — restricting to an absolute Docker binary path and a local `unix:///` socket is a reasonable containment measure preventing accidental remote-daemon targeting; no issue.
- `provision.py:22-27` — pre-flight rejection of unexpected/symlinked build-context entries is a sound anti-tamper check for the "replay must not extract unrelated archives" invariant.

### Out of scope for this static review

- Correctness of the pinned SHA-256 values in `sources.json` (would require network fetch) — not verifiable here; flagged as a limitation of this review, not a defect in the code.

**Summary:** one concrete P1 (likely-nonfunctional `Config` equality check in `verify.py:20-22`, fails closed) and two minor P2 notes (extraction hardening, version-metadata clarity). No P0s — no unauthorized execution, no injection vectors (`subprocess.run` uses argument lists, no `shell=True`), and privilege/hardening posture for both build and verification stages is appropriate for the stated host-provisioning, non-sandbox scope.
