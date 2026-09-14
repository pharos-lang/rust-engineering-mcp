# W01b — Fix the findings on the M6 provisioning package (W01)

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`). Role: implementation worker, same file ownership as W01. Orchestrator: Claude Fable 5.1. Do not rebuild the image unless a change touches `build.sh`/`Dockerfile`/`provision.py`'s context assembly (it does — see item 4 and 5 — so you must rebuild once at the end and regenerate the receipt).

## Inputs

- The independent review `docs/validation/M6/delegation/V01-review-provisioning/claude-sonnet-5-review.md` (verdict Block: two P2, six P3). The orchestrator accepted every finding below; where it says "accepted with adjustment", follow the adjustment.
- The orchestrator's own findings (items 8–9), which the reviewer could not see because they need files outside the diff.
- The original package `docs/validation/M6/delegation/W01-provisioning/prompt-header.md` for the constraints that still apply (stdlib only, no `/tmp` literals, no argv-derived paths opened directly, receipt honesty, do not commit).

## Changes (all mandatory)

1. **P2 — `provision.py --output` taint.** Replace the free `--output` path with the project pattern: a constant `OUTPUT_DEFAULT = ROOT / "target/m6-provisioning"` and `beside_default(OUTPUT_DEFAULT, value)` (copy the helper from `scripts/build-m6-runtime.py`, or import-free duplication is acceptable in a fixture script) so only the basename of any argument is honoured and `shutil.rmtree` can never leave `target/`. Adjust `scripts/build-m6-runtime.py` so what it passes still resolves to the same directory it later reads (`context_root`). Add a test that a traversal value cannot escape.
2. **P2 — tar-guard tests.** In `fixtures/rust-runtime/m6/test_provision.py` add discriminating tests for `validate_component_archive`: hardlink member (`tarfile.LNKTYPE`), character device (`CHRTYPE`), block device (`BLKTYPE`), fifo (`FIFOTYPE`), duplicate member name, and an archive whose root directory is right but contains a member with a backslash. Each test must fail if its specific `raise` were removed (assert on the exact message fragment).
3. **P3 — size bounds.** Add per-member (`MAX_MEMBER_BYTES = 64 MiB`) and aggregate (`MAX_ARCHIVE_BYTES = 512 MiB`) caps on `member.size` in `validate_component_archive`, with a test for each; the real inputs (8.7 MB and 5.7 MB compressed; rust-src ~40 MB uncompressed) are far below.
4. **P3 — off-PATH self-check in `build.sh`.** Loop over every directory of the final image `PATH` (`/usr/local/sbin /usr/local/bin /usr/sbin /usr/bin /sbin /bin`) plus `/opt/rust/bin` and assert `test ! -e "$dir/rust-analyzer"`.
5. **P3 — glob guard in `build.sh`.** Before each `ln -s`, `test -e "$shared_object"` (fail the build otherwise) so an unexpanded glob can never create a dangling symlink; also assert afterwards that `/opt/analyzer/lib` contains at least one `librustc_driver-*.so` symlink.
6. **P3 — ADR-082 wording.** Say the manifest sha256 constant in `provision.py` *equals* the value recorded in `fixtures/rust-runtime/sources.json` (cross-check by the reader), not that the script reads it from there.
7. **P3 — naming and test gaps.** Rename `NEW_COMPONENTS` in `scripts/build-m6-runtime.py` to `ANALYZER_BINARIES` (or similar) and add a `validate_context` test with a stray directory.
8. **Orchestrator finding — stage counts in `docs/ci.md`.** `scripts/gate.py` already ran 23 core stages before M6 (the list in `ci.md` omitted `m5-helper-guest-clippy`; the M5 receipts say 23/23 and 38/38). With the two M6 stages the truth is **25 core / 40 full**. Fix the numbers and add `m5-helper-guest-clippy` to the enumerated list. Do not touch any other number in the file.
9. **Orchestrator finding — SonarCloud coverage.** Add `scripts/build-m6-runtime.py` to `sonar.coverage.exclusions` in `sonar-project.properties` (host/Docker-only script; precedent `scripts/build-m5-runtime.py` on that same line). Keep the two coverage invocations in `.github/workflows/sonarcloud.yml`; `fixtures/` is outside `sonar.sources`, so they are harmless.

## Verification (targeted)

```text
python3 -B -m unittest fixtures/rust-runtime/m6/test_provision.py
python3 -B scripts/test-m6-provisioning.py
python3 -B scripts/test-gate-reporting.py
python3 -B scripts/build-m6-runtime.py        # once, at the end; regenerates docs/validation/M6/provisioning.json
python3 -B scripts/docs-hygiene.py links-check
python3 -B scripts/docs-hygiene.py verify-inventories
```

The rebuilt image id will differ from `sha256:64b2e614…`; that is expected — the receipt is the authority. Report the new id and the new `rust_analyzer_version` line.

## Report (mandatory headings)

Task / Result / Files changed / Tests executed / Evidence / Risks / Decisions / Open issues.
