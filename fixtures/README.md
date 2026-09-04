# M0-10 Rust fixture corpus

Run with Python 3.11+ and the real Rust/Cargo/Clippy 1.98.1 toolchain:

```text
python3 scripts/test-fixtures.py /absolute/path/to/root --cargo /absolute/path/to/toolchain/bin/cargo
```

The root argument contains `fixtures/` and must be a trusted reviewed checkout. The harness contains a closed allowlist of eight reviewed
benign fixture directories. It never discovers projects recursively and never
invokes Cargo on `vulnerable-dependency` or `security/malicious-build-script`.
Every standalone root declares `[workspace]`; the literal workspace fixture owns
its two members. All compile/test commands use `--locked --offline`.

| Fixture | Oracle on Rust 1.98.1 |
| --- | --- |
| valid-basic | Test executes: 1 passed, 0 failed |
| borrow-error | Check fails: E0502, primary `src/lib.rs:4` |
| lifetime-error | Check fails: E0597, primary `src/lib.rs:5` |
| clippy-warning | Clippy succeeds and emits clippy::useless_vec at `src/lib.rs:3` |
| unsafe | Compilation fails due to deny(unsafe_code) at `src/lib.rs:3` |
| vulnerable-dependency | Static lock/advisory provenance checks only; see its README |
| workspace | Inheritance and path dependency compile; consumer test executes |
| feature-conflict | Default/left/right pass; left+right fails at `src/lib.rs:2` with fixed compile_error marker |
| build-script | Cargo emits build-script-executed; OUT_DIR contains the exact generated source; generated test executes |
| security/malicious-build-script | NEVER RUN HOST; source-only future gateway adversary |

Each command has a 30 second wall deadline and separate 512 KiB stdout/stderr limits.
A fresh temporary HOME, CARGO_HOME, TMPDIR and target directory are removed on exit.
The child environment is constructed from an allowlist, including the real toolchain
and `/usr/bin:/bin` for platform linker tools. No host environment is inherited.
Timeout/output overflow kills the process group. This is a harness for reviewed,
trusted fixtures, **not an OS sandbox or a safe runner for arbitrary project roots**.
`--offline` prevents Cargo dependency fetching; it is not network isolation for build
scripts. The supplied benign build script writes only its Cargo-provided OUT_DIR.
The malicious source is deliberately outside the host execution allowlist.

Assertions inspect Cargo JSON diagnostic codes and primary source spans rather than
rendered English diagnostics. Unit-test outcomes use libtest's fixed success summary.
The harness also requires a consistent Cargo build-finished event. Fixture source line
changes must update the associated expected spans deliberately. Optimized Python (`-O`/PYTHONOPTIMIZE) is rejected.

No generated target trees, downloaded packages or compiler artifacts belong in this
corpus. Lockfiles are fixed; the audit-only RSA lock intentionally is not a full build
lock. No cargo-audit/RustSec matching engine or vulnerability exploit was executed.

`corpus-sha256.json` is checked before Cargo; it detects accidental/unreviewed changes
and added files in the fixture directories. It is not a publisher signature and
does not make an untrusted checkout safe. Source changes require deliberate receipt
updates. The principal independently re-fetched the pinned upstream RSA advisory
and registry checksum; the offline harness subsequently checks consistency.
