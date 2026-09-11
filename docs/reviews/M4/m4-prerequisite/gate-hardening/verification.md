# P3-B/C/D/E gate hardening verification

Generated at `2026-09-07T22:54:50Z` against Git
`c66a3704e1ad290603a3c1d10413df90d15c2b03`.

## Qualified changes

- The Sonar property parser trims keys and values, so whitespace around `=` cannot
  hide an exclusion.
- Both `sonar.exclusions` and `sonar.coverage.exclusions` are checked against the
  complete dynamic inventory from `crates/*/src/**/*.rs`.
- The policy oracle has no fixed list of Rust filenames.
- The real-Git fixture uses a minimal environment and neutralizes host global and
  system configuration. Its adversarial input includes `GIT_DIR`,
  `GIT_INDEX_FILE`, and a global `core.excludesFile` matching `*.properties`.
- The documented SonarCloud unittest total is 79.

## Input hashes

```text
064754859d261a01c787f929c4976fec6a45ea8911955e2d3a8c057d5d764ddb  scripts/test-gate-reporting.py
b0a3f602c7c6507ee089edce4a4d443deba831ffb9bfd1efd4b20c64c110b0cc  docs/ci.md
1192befea592a36b84bbbd0995eab5b32f89a59cc4f6e05e98123d7be5116e41  sonar-project.properties
d4dbd03b4b3d23795f9d6346c64ee1d1a0f1eecd362ca9875ddfb684b8543409  scripts/gate.py
```

## Reproduction

Environment: `Python 3.14.7`; `git version 2.50.1 (Apple Git-155)`.

```text
python3 -m py_compile scripts/test-gate-reporting.py
PASS

python3 scripts/test-gate-reporting.py
Ran 13 tests in 0.303s
OK

python3 scripts/test-coverage-reports.py
Ran 3 tests in 0.003s
OK

python3 scripts/test-release-artifact.py
Ran 11 tests in 0.007s
OK

python3 scripts/test-release-smoke.py
Ran 9 tests in 0.013s
OK

python3 scripts/test-codex-model-qualifier.py
Ran 39 tests in 52.912s
OK

python3 scripts/test-public-export.py
Ran 4 tests in 0.012s
OK

python3 scripts/check-architecture.py
PASS domain/application dependency and IO boundaries, sole process gateway,
offline engine defaults, memory-only model adapter

git diff --check -- scripts/test-gate-reporting.py docs/ci.md
PASS
```

The six SonarCloud unittest suites therefore execute
`3 + 13 + 11 + 9 + 39 + 4 = 79` tests. `scripts/check-architecture.py` is also
invoked by the SonarCloud workflow, but it is an assertion-based architecture
check rather than a unittest suite and contributes no `Ran N tests` count.

The local Python does not have Coverage.py installed, so the suite entrypoints
were executed directly. No dependency was installed.
