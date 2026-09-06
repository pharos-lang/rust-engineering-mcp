# Package Q02 — Unblock and qualify M3-03 coverage, then run the complete M3 runtime gate (integrator Sol, resumed; sole Docker owner)

Your Q01 escalation is answered. Observed blocker: `cargo llvm-cov --no-report` must build and run instrumented test binaries in a target directory that also has to survive into the separate `report` container, but the persistent volume profile forbids execution there, so the run exits 101 with `Permission denied`.

## Orchestrator decision (authorized, with limits)

Introduce a **dedicated coverage build/target volume** that is the only mount in the product allowed to hold runnable build output across the two coverage phases. All of these constraints are mandatory:

- A named tmpfs volume used **only** by `rust.coverage`'s run and report phases, created per job and removed by the same cleanup path as the other volumes (quarantine on uncertainty).
- Options identical to the existing approved profile except for the single flag that currently prevents running the freshly built test binary; keep `nosuid`, `nodev`, `uid=65534`, `gid=65534`, `mode=0700`, an explicit bounded `size=` (propose a value derived from the observed instrumented build, e.g. 512m, and justify it) and an explicit `nr_inodes=`.
- Mounted read-write in the run phase and **read-only** in the report phase (llvm-cov only reads binaries and profraw data).
- The source volume, the report/artifact volume, `/tmp` and every other mount of every phase of every tool keep their current options unchanged. Nothing else changes.
- No change to seccomp, network (`none`), user, read-only root filesystem, dropped capabilities, or the PID/CPU/memory caps.
- The applied-mount verifier must assert the exact shape of this volume for the coverage phases, assert that no other phase mounts it, and assert that every other volume keeps its current restrictive options. The gateway configuration fingerprint must include the change, and recalibration follows.

Rationale to record: running project code inside the sandbox is already the qualified behaviour of every R2 operation (ADR-037), and ADR-056 already granted `cargo fix` a runnable `/target` tmpfs inside a single container. The delta here is only that the same kind of area must persist across two containers of one job, scoped to the coverage tool and destroyed at cleanup. This is an incremental, documented mount decision, not a new privilege class. If while implementing you find a smaller option that still guarantees one run feeding three formats (for example the report phase reading a copy while the run phase keeps a container-local area, or a single-container flow), prefer it and explain why.

## Work

1. Write `docs/adr/ADR-065-coverage-target-volume.md` (Status: Proposed 2026-09-06, authorized by the orchestrator; Context / Decision / Alternatives considered / Consequences / Sources) covering exactly the above, the rejected alternatives (three independent runs; rebuilding in the report container; a shell or multi-command entrypoint; widening the shared profile for every tool) and the negative controls below.
2. Implement it: gateway phases, volume lifecycle, verifier assertions, fingerprint, recalibration.
3. Negative controls as real tests: a binary planted on the source volume still cannot be run; a binary planted on the report/artifact volume still cannot be run; the coverage volume is absent from every non-coverage phase; the volume is gone after cleanup; network remains denied inside the coverage phases.
4. Run the **complete** M3 runtime Docker gate (all 55 selections: nextest 19, coverage 8, semver 18, mutation 10) serially in a fresh output directory until it is green or a real defect stops it. Preserve every failed attempt as `M3-runtime-attemptN.json` and write the passing receipt to `docs/validation/M3-runtime.json`.
5. Calibrate coverage against the real plugin: llvm-cov identity, the exact JSON consumed, the known-counts oracle (hand-derived counts must match the observed ones; if they do not, investigate the fixture before touching the oracle), LCOV/HTML derived from the same profdata, the zero-denominator rule and the shared-file dedupe. Update `docs/validation/M3-03.md`, the matrix row, the measured parts of ADR-062 and its Open issues.
6. Re-run `scripts/test-rust-execution.py` (20 selections) because the gateway fingerprint changed, refreshing `docs/validation/M3-rust-security.json` and keeping the previous receipt as history.
7. Final workspace gates: `cargo fmt --check`, `cargo check/clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo test --workspace --locked --offline`, `python3 -B scripts/check-architecture.py`. Check Docker hygiene (zero owned containers and volumes) after each gate.

## Rules

Branch `ai/m3-quality`; no commits, merge, push, installs, downloads, or image build/pull/delete; you are the sole Docker owner. A Claude reviewer is reading the contract surface concurrently in read-only mode; ignore it. Keep all 23 tool snapshots byte-identical unless a calibration legitimately changes a schema, and if so say it explicitly. Never mark a skip as a pass, and never change containment beyond the decision above without escalating again.

Delivery: Task, Result (coverage qualified, or blocked with the reproducible condition), Files changed with SHA-256, Tests executed (each gate with command, exit code, counts, duration, receipt path), Calibration table, Evidence (oracle → test name), Risks, Decisions, Open issues.
