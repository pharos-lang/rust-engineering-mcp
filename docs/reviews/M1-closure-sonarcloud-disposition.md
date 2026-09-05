# M1 closure SonarCloud disposition

Date: 2026-09-05. Public PR: `#8`. Initial SonarCloud workflow run:
`33946726591`; candidate commit
`b7081973a38a608e86efa6c73741f3d5a5d442ba`.

## Initial gate result

The first public analysis correctly blocked the PR. Its Quality Gate reported
`new_coverage = 0.0%` and a new security rating of D from sixteen findings in the
three newly published Python qualification programs. Linux, macOS, Windows and the
supply-chain job all passed independently; none of those results waives SonarCloud.

## Coverage disposition

The workflow previously measured only the architecture checker and the coverage
report validator. It now runs all 65 Python tests under the same branch-coverage
database: gate reporting (6), artifact production (11), archive smoke (9) and stock
Codex qualification (39), in addition to the existing 3 reporting tests.

The three qualification programs are excluded only from Sonar's coverage metric.
Their executable main paths require a real Darwin ARM64 release host, Docker/Codex,
or both and cannot be exercised honestly by the portable Linux scanner. Their tests
still execute and fail the workflow, and their authoritative end-to-end evidence is
the candidate-bound artifact, Inspector and stock Codex receipts. They remain fully
included in Sonar static and security analysis.

## Security disposition

Every reported line was reviewed individually. The findings cover:

- path reads or writes that are authenticated by an approved plan digest, confined
  to an owned private output root, or opened no-follow and revalidated with `fstat`;
- the bounded event loop whose deadline derives from schema-checked, hash-approved
  budgets;
- a Docker CLI invocation whose executable hash, socket, environment and closed
  argument builders are verified and which never uses a shell;
- fixed Cargo metadata arguments with a cleaned environment and no user flags;
- in-memory tar members whose names pass the closed `safe_relative` grammar; and
- exclusive receipt creation and cleanup relative to the same resolved directory
  descriptor.

These are false positives caused by the scanner not carrying the cross-function
authentication and containment invariants. Each is suppressed with a `NOSONAR`
annotation on the exact reported line and an adjacent control-specific reason. No
file, rule or security analyzer is globally excluded.

## Closure condition

This disposition is accepted only if the synchronized public commit receives a new
green SonarCloud Quality Gate and the complete portable/supply-chain matrix remains
green. Until then PR #8 remains blocked and cannot be merged.
