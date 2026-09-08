# Miri classification oracle fixtures

These first-party fixtures exercise a narrow, empirical classification design on
the exact M4 image. They are not product integration or Execution Gateway
qualification.

Run from the repository root:

```text
python3 fixtures/m4-runtime-oracles/miri_classification.py
```

The script creates only nonce-owned Docker containers and a 32 MiB tmpfs JUnit
volume. Every guest has no network, a read-only root filesystem and source bind,
uid/gid 65534, 128 PIDs, 1 GiB RAM, one CPU, and bounded work/tmp/output. Cleanup
removes and verifies every owned object, then compares the complete Docker
container/volume inventory with the initial inventory.

`nextest.toml` keeps Miri stdout available during nextest's libtest listing phase.
Its product-owned run wrapper sets `-Zmiri-mute-stdout-stderr` only while executing
each selected test. Thus nextest can inventory tests, while interpreted test output
cannot forge the Miri JSON retained in a failed testcase's JUnit `system-err`.

Expected observations:

| Fixture | Expected classification |
| --- | --- |
| `benign-forged` | one pass plus an ordinary `test_failure`; its forged UB marker is absent |
| `clean` | `clean`, with `cfg(miri)` active |
| `uaf`, `uninit`, `alias`, `race` | `undefined_behavior` from an error-level Miri JSON diagnostic |
| `ffi` | `unsupported_operation` from an error-level Miri JSON diagnostic |
| `compile-fail` | `compile_failure` during nextest listing |
| `empty` | `incomplete_no_tests`, never clean |
| `ignored` | `incomplete_skipped_only`, never clean |

The results directory stores each bounded raw stdout/stderr inside its case JSON,
the raw JUnit document separately, SHA-256 hashes, argv/environment, termination,
and cleanup/source-integrity facts. `summary.json` binds the image, image config,
nightly commit, nextest version, executable hashes, sysroot tree hash, script hash,
and fixture tree hash.

This oracle deliberately contains no build scripts, proc macros, custom harnesses,
or doctests. Native compile-time code shares compiler output and invalidates the
diagnostic-origin assumption; such projects require rejection before this mode or
a separately qualified trusted boundary. Nextest also cannot cover doctests or
cross-test data races, so the race fixture creates both competing threads inside a
single test. A clean observation covers only the selected execution and is not a
proof that the project is free of undefined behavior.
