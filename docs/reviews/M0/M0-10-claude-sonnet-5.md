# Revisión M0-10

Claude Code2.1.259, Sonnet5 Medium; safe/restricted, sin tools/MCP/persistencia.

Reviewed the fixture corpus, README, and `scripts/test-fixtures.py` harness. Four concrete findings, ordered by severity:

## 1. `fixture-sha256.json` is never verified — dead integrity control (High)
`scripts/test-fixtures.py` never reads or imports `fixture-sha256.json` — `hashlib` is only used inside `audit_input()` to hash `RUSTSEC-2023-0071.md`. No code path opens the manifest, iterates its entries, or compares them against on-disk fixture files. `fixtures/README.md` also never mentions it.

Given the stated threat model ("reviewed, trusted fixtures... never execute malicious/vulnerable on host"), a committed hash manifest is exactly the kind of tamper-evidence you'd want checked before the harness trusts a fixture directory and (for the benign set) actually runs `cargo` against it. Right now it's decorative: someone could hand-edit any benign fixture's `src/lib.rs` (e.g. weaken `unsafe/src/lib.rs`, or add real filesystem/network calls to a "benign" fixture) and the harness would run it with zero detection, since nothing checks the file against `fixture-sha256.json` before invoking `cargo`.

Either wire it in (verify every path in the manifest against SHA-256 before any `compiler_case`/`audit_input` call, fail closed on mismatch or unlisted file) or remove it — an unenforced integrity manifest is worse than none, because it looks like a control that isn't one.

## 2. `audit_input()`'s "verification" is circular (Medium — misleading evidence)
`scripts/test-fixtures.py` (`audit_input`, ~lines 78–93) checks:
- `sha256(RUSTSEC-2023-0071.md) == provenance.json["advisory_sha256"]`
- `Cargo.lock`'s `rsa` checksum `== provenance.json["registry_checksum"]`

All four values (`RUSTSEC-2023-0071.md`, `provenance.json`, `Cargo.lock`) are committed together in the same fixture directory by the same author/commit. This proves internal self-consistency, not fidelity to the real upstream advisory-db commit `d674d8e9...` or the real crates.io index entry for `rsa@0.9.6`. The README's factual claims ("exact copy from the official RustSec advisory-db at commit...", "checksum was read from https://index.crates.io/3/r/rsa") are asserted in prose only — there is no fetch-and-diff step, static or otherwise, anywhere in the harness. The printed `"verification": "static pinned audit input only; no Cargo invocation"` is technically true but easy to misread as "we verified the pin is correct," when what's actually verified is only "the three files we wrote agree with each other." Flag for the reviewer: someone needs to independently re-fetch both URLs and diff/recompute hashes before trusting this fixture as ground truth for a future audit adapter (M1).

## 3. Unhandled `TimeoutExpired` on early-pipe-EOF (Low–Medium, robustness)
`run()` (`scripts/test-fixtures.py` ~lines 21–46): the deadline is only enforced *inside* the selector loop, which exits as soon as both stdout and stderr hit EOF — not when the process actually exits. If a child closes its output pipes but keeps running (e.g. a build script spawning a lingering grandchild, or a hung linker), and the wall deadline has already elapsed, `process.wait(timeout=max(0.001, deadline - time.monotonic()))` raises `subprocess.TimeoutExpired`, which is not caught. The `finally` block still kills the process group (so no security/hang issue), but the clean `RuntimeError("...exceeded 30 second deadline")` path is bypassed — the harness would crash with a raw traceback instead of the intended, parseable timeout diagnostic. Worth a `try/except subprocess.TimeoutExpired` around that `wait()` that re-raises the same `RuntimeError` used elsewhere for consistency.

## 4. Misleading framing in `fixtures/README.md` for the `unsafe` fixture (Low)
"unsafe | Check fails: unsafe_code at `src/lib.rs:3`; **never executes unsafe code**" reads as if the harness deliberately prevents execution (echoing the real prevention mechanism used for `security/malicious-build-script`). In reality it never executes because `#![deny(unsafe_code)]` makes the crate fail to *compile* — there's no unsafe-code-specific guard in the harness itself; any fixture that failed to compile for any reason would equally "never execute." Worth rewording to something like "fails to compile due to `deny(unsafe_code)`" so a reviewer doesn't conflate this with the intentional gateway/allowlist protections used for the malicious fixture.

Everything else checked out: the 8-entry `BENIGN` allowlist matches the README's claim of 8 reviewed directories, the 11 compiler cases and their expected diagnostic codes/spans/lines are consistent with real rustc/clippy behavior for the given source, `malicious-build-script` is genuinely unreachable from `test-fixtures.py` (no recursive discovery, no allowlist entry), and the build-script fixture's OUT_DIR assertions are correctly scoped.

## Disposición principal

1. El hash staging era un recibo de handoff, no una autenticación del código.
   Se integra ahora corpus-sha256.json y verificación pre-Cargo de todos los archivos,
   extras y symlinks. No convierte un checkout malicioso en seguro: tanto script
   como manifest siguen siendo código revisado del host. Severidad High no aplica
   como escape de una frontera que nunca se afirmó, pero el control de drift es útil.
2. El principal re-fetched ambos URLs oficiales y comparó advisory byte-identical y
   checksum de rsa0.9.6. Recibo M0-10-upstream.json. Se conserva el check offline de
   consistencia y se declara claramente que RustSec matching será M1.
3. TimeoutExpired tras EOF ahora se convierte al mismo RuntimeError; finally conserva
   killpg y reap. No se afirma containment de descendientes desacoplados del grupo.
4. README unsafe describe fallo de compilación por deny(unsafe_code), no un sandbox.

Después de la revisión se añadieron al adversario fuente dos probes finitos:256
líneas de1024bytes y un hijo de60segundos creado con programa/argumento fijos.
Principal revisó ese cambio; no se compiló/ejecutó el adversario en host. El gate
M0-12 revisará el alcance de esta evidencia. No se afirma segunda revisión externa.
