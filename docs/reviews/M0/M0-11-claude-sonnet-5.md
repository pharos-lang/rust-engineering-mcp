# Revisión M0-11 CI local

Claude Code2.1.259, Sonnet5 Medium; safe/restricted, sin tools/MCP/persistencia.

## Review: M0-11 Local CI (ADR-029, gate.py, deny.toml, verify scripts)

Confirmed compliant claims (no action needed):
- **No cross-platform overclaim**: `docs/ci.md` matrix marks Linux (both arches), Windows, and macOS x86_64 as "Sin ejecutar," and ADR-029 explicitly calls them "unverified and unadvertised." Matches the actual gate logic, which hard-fails full-gate on non-macOS/ARM64.
- **License gate correctly deferred**: `deny.toml` has no `[licenses]` table, and `scripts/gate.py`'s `cargo deny check` invocation explicitly lists only `advisories bans sources` — `licenses` is never invoked, so nothing can spuriously "pass." Matches the ADR-029/docs/ci.md statement that `deny licenses` is an explicit M1 gate.
- `paste` unmaintained warning and duplicate-version warnings are consistent between `deny.toml` and `verify-vendor.py`'s deny-list assertions.

### Findings

**1. [Medium] Reproducibility — inconsistent `cargo` binary resolution**
File: `scripts/gate.py`
Evidence: `check`/`clippy`/`test`/`doctests` invoke bare `['cargo', ...]` resolved via inherited `PATH`, while the `cargo-fixtures` step explicitly resolves `cargo=subprocess.check_output(['rustup','which','cargo'],...)`. Only `rustc --version` is pinned to `1.98.1`; `cargo` itself is never version-checked.
Impact: if PATH's `cargo` differs from `rustup which cargo` (shims, `asdf`/`direnv`, multiple toolchains), the fixture corpus runs under a different toolchain than the rest of the gate, silently breaking the single-toolchain guarantee.
Recommendation: resolve `cargo` once via `rustup which cargo` and reuse for every step, or add an explicit `cargo --version` check alongside the `rustc` check.

**2. [Medium] Failure propagation — late-fail wastes the whole core run before rejecting unsupported platforms**
File: `scripts/gate.py`
Evidence: `if os.name=='nt': raise RuntimeError(...)` runs after `fmt/check/clippy/test/doctests/architecture/vendor` have already executed and been recorded `passed` in the JSON report; similarly the `full`-mode `if sys.platform!='darwin' or platform.machine()!='arm64'` and the required-env-var checks run only after the entire core suite (including `audit`/`deny`) has already completed.
Impact: exit code and top-level `status` still correctly end up `failed` (no false certification), but a reader parsing individual `steps[].status` rather than the top-level field could mistake a partial run for a passing platform, and unsupported-platform/full-mode invocations pay the full core-suite runtime before failing.
Recommendation: move the `os.name=='nt'` guard and the full-mode platform/env-var prerequisite checks to the very top of `main()`, before any `run()` call.

**3. [Low] Doc gap — `rustup` is an undocumented hard dependency**
Files: `docs/ci.md`, `scripts/gate.py`
Evidence: prerequisites list in `docs/ci.md` says "Rust/Cargo1.98.1+rustfmt+Clippy" but doesn't mention `rustup`; `gate.py` calls `rustup which cargo` with no pre-check, unlike `cargo-audit`/`cargo-deny` which get an explicit `shutil.which` guard and a clear "provision explicitly; no install attempted" message.
Recommendation: add a `shutil.which('rustup')` pre-check with the same explicit error message, and list `rustup` in `docs/ci.md` prerequisites.

**4. [Low] Idempotency — built probe image is never removed**
File: `scripts/test-execution.sh`
Evidence: `trap 'rm -rf "$TASK_CONTEXT"' EXIT HUP INT TERM` cleans the build context, but the tagged image `rust-mcp-probe:m0` is left resident in the Docker daemon; each run reuses the same tag against Docker's layer cache rather than a byte-for-byte fresh build.
Recommendation: either remove the image after the test run or document that repeated gate runs are cache-assisted rebuilds, not clean-room builds, if that matters for the evidence story.

**5. [Info — needs confirmation] Untracked `artifact-adapter` crate not yet reflected in architecture/vendor tooling**
Files: `scripts/check-architecture.py`, `crates/artifact-adapter/` (untracked per git status)
Evidence: `check-architecture.py`'s per-crate dependency allowlist and I/O-boundary sweep are generic over `crates/*/src/**/*.rs`, so the new crate is automatically swept by the `Command::new` check and by the domain/application I/O check (only if `domain`/`application` literally appear in the path, which they won't for `artifact-adapter`). That's consistent with how other adapters (`execution-adapter`, `semantic-adapter`) are handled, but since `crates/domain/src/artifact.rs` and `crates/application/src/artifact.rs` are new/untracked, confirm they don't introduce dependencies outside the `allowed` map (`domain: {'serde'}`, `application: {'domain'}`) before relying on `scripts/gate.py core` as green evidence for M0-11 — this file review did not include the contents of those two new source files.

**Net assessment**: no evidence of overclaiming or fabricated pass state was found — failure handling correctly fails closed and the top-level `status`/exit code cannot be spoofed into a false pass. The issues above are about *when* failure is surfaced (late vs. early) and *tooling-resolution consistency*, not about incorrect gating logic.

## Disposición principal

1. Corregido: rustup which --toolchain1.98.1 consulta un toolchain ya instalado;
   Cargo/rustc reales se verifican y PATH/RUSTC quedan fijados para todos los hijos.
   Gate filtra entorno a una allowlist de desarrollo; no admite RUSTC_WRAPPER ni
   RUSTFLAGS heredados. No se afirma seguridad frente a un host/toolchain malicioso.
2. Corregido: plataforma y variables full se validan antes de cualquier run();
   el control negativo confirma statusfailed/stepsvacíos sin empezar core.
3. rustup documentado y preflight explícito; no se instala nada si falta.
4. Se conserva imagen de probes deliberadamente para inspección/reuso. Documentado
   build con cache, sin claim clean-room y sin borrar imagen aprobada del host.
5. Domain soloSerde, application soloDomain también trasArtifactStore. El check
   real recorrió todos los sources/metadata; artifact/semantic adapters además
   tienen controles textuales de ausencia de filesystem. No reemplazan revisión.

Mejoras principales adicionales: test-semantic usa target/semantic-compat por
omisión (override explícito posible), check/Clippy con workspace all-features y
pruebas de ejecución semántica focalizadas. Runtime tests conservan env limpio,
perfil networkdeny y model/native hashes. No se ejecuta toda la suite filesystem
bajo ese perfil/TMPDIR inexistente. Core173tests+doctest pasó desde entrypoint real;
full será la evidencia de cierreM0-12. Sin segunda aprobación externa implícita.
